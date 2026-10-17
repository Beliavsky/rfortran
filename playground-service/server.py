"""HTTP gateway. Public jobs run only in restricted disposable Docker containers."""
import argparse
from collections import OrderedDict
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import ipaddress
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time
from urllib.parse import urlsplit
import uuid

from protocol import JOB_SECONDS, MAX_BODY, validate
from worker import execute, run_job

ROOT = Path(__file__).resolve().parent


def container_command(image, name):
    return ["docker", "run", "--rm", "--name", name, "--network", "none", "--read-only",
            "--user", "65532:65532", "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
            "--memory", "256m", "--memory-swap", "256m", "--cpus", "1", "--pids-limit", "64",
            "--ulimit", "nofile=64:64", "--ulimit", "fsize=67108864:67108864", "--ulimit", "core=0",
            "--tmpfs", "/work:rw,exec,nosuid,size=64m,mode=1777",
            "--tmpfs", "/tmp:rw,exec,nosuid,size=64m,mode=1777", "-i", image]


class Gateway:
    def __init__(self, image=None, compiler=None, origins=()):
        self.image, self.compiler, self.origins = image, compiler, set(origins)
        self.slots = threading.BoundedSemaphore(2)
        self.rate_lock = threading.Lock()
        self.clients = OrderedDict()
        self.metadata = {"compiler": "rfort", "mode": "local" if compiler else "container",
                         "max_files": 8, "run_seconds": 3, "compile_seconds": 20,
                         "image": image if image else None}
        if compiler:
            self.metadata["version"] = subprocess.check_output([compiler, "--version"], timeout=10, text=True).strip()
        else:
            inspected = subprocess.run(["docker", "image", "inspect", image], check=True, capture_output=True, text=True, timeout=15)
            labels = json.loads(inspected.stdout)[0].get("Config", {}).get("Labels") or {}
            self.metadata["commit"] = labels.get("org.opencontainers.image.revision", "unknown")

    def allow(self, address):
        now = time.monotonic()
        with self.rate_lock:
            tokens, previous = self.clients.pop(address, (4.0, now))
            tokens = min(4, tokens + (now - previous) / 6)
            allowed = tokens >= 1
            self.clients[address] = (tokens - 1 if allowed else tokens, now)
            while len(self.clients) > 2048:
                self.clients.popitem(last=False)
            return allowed

    def run(self, request):
        if self.compiler:
            return run_job(request, self.compiler)
        payload = json.dumps(request)
        name = "rfort-job-" + uuid.uuid4().hex
        try:
            result = execute(container_command(self.image, name), ROOT, payload, JOB_SECONDS)
        finally:
            subprocess.run(["docker", "rm", "-f", name], capture_output=True, timeout=10)
        if result["timed_out"] or result["output_limited"]:
            return {"error": "Job exceeded its total time or output limit."}
        try:
            value = json.loads(result["stdout"])
        except (ValueError, TypeError):
            self.worker_error(result)
            raise RuntimeError("Execution worker unavailable; check service logs.") from None
        return value

    @staticmethod
    def worker_error(result):
        import sys
        print(f"Worker failed (exit {result['exit_code']}): {result['stderr'][:1000]}", file=sys.stderr, flush=True)


class Handler(SimpleHTTPRequestHandler):
    server_version = "rfort-playground"

    def setup(self):
        super().setup()
        self.connection.settimeout(10)

    @property
    def gateway(self):
        return self.server.gateway

    def end_headers(self):
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self' https: http://localhost:* http://127.0.0.1:*; object-src 'none'; base-uri 'none'; frame-ancestors 'none'")
        origin = self.headers.get("Origin")
        if origin in self.gateway.origins or origin == self.own_origin():
            self.send_header("Access-Control-Allow-Origin", origin)
            self.send_header("Vary", "Origin")
        super().end_headers()

    def own_origin(self):
        return "http://" + self.headers.get("Host", "")

    def permitted(self):
        # Local mode must not be reachable through a DNS rebinding hostname.
        host = urlsplit("http://" + self.headers.get("Host", "")).hostname
        if self.gateway.compiler and host not in ("127.0.0.1", "localhost", "::1"):
            self.respond(403, {"error": "Local mode requires a loopback hostname."})
            return False
        origin = self.headers.get("Origin")
        if origin and origin not in self.gateway.origins and origin != self.own_origin():
            self.respond(403, {"error": "Origin is not allowed."})
            return False
        return True

    def respond(self, status, value):
        data = json.dumps(value).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        try:
            self.wfile.write(data)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def do_OPTIONS(self):
        if self.permitted():
            self.send_response(204)
            self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
            self.send_header("Access-Control-Allow-Headers", "Content-Type")
            self.end_headers()

    def do_GET(self):
        if not self.permitted():
            return
        if self.path == "/api/health":
            return self.respond(200, self.gateway.metadata)
        if self.path.startswith("/api/"):
            return self.respond(404, {"error": "Unknown endpoint."})
        # Serve only the new frontend, never repository sources or directory listings.
        if self.path.split("?", 1)[0] not in ("/", "/index.html", "/style.css", "/app.mjs", "/examples.mjs", "/model.mjs", "/config.json"):
            return self.respond(404, {"error": "Not found."})
        return super().do_GET()

    def do_HEAD(self):
        self.send_error(405)

    def do_POST(self):
        if not self.permitted():
            return
        if self.path != "/api/compile":
            return self.respond(404, {"error": "Unknown endpoint."})
        try:
            length = int(self.headers.get("Content-Length", "-1"))
            if self.headers.get("Transfer-Encoding") or not 0 <= length <= MAX_BODY:
                return self.respond(413, {"error": "Request too large or missing Content-Length."})
            if self.headers.get_content_type() != "application/json":
                return self.respond(415, {"error": "Use application/json."})
            raw = self.rfile.read(length)
            if len(raw) != length:
                raise ValueError("Incomplete request.")
            request = validate(json.loads(raw))
        except (ValueError, UnicodeError) as error:
            return self.respond(400, {"error": str(error)})
        if not self.gateway.allow(self.client_address[0]):
            return self.respond(429, {"error": "Too many jobs. Please wait a few seconds."})
        if not self.gateway.slots.acquire(blocking=False):
            return self.respond(429, {"error": "Both execution slots are busy. Please try again."})
        try:
            self.respond(200, self.gateway.run(request))
        except Exception:
            self.log_error("Execution worker failed")
            self.respond(503, {"error": "Execution service unavailable."})
        finally:
            self.gateway.slots.release()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8080)
    parser.add_argument("--origin", action="append", default=[])
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--image", help="Prebuilt sandbox image; required for public execution")
    mode.add_argument("--local-compiler", help="Trusted local testing only; never a public sandbox")
    args = parser.parse_args()
    if args.local_compiler:
        if args.bind != "localhost" and not ipaddress.ip_address(args.bind).is_loopback:
            parser.error("Local compiler mode may bind only to loopback.")
        if args.origin:
            parser.error("Local compiler mode cannot enable cross-origin requests.")
    with tempfile.TemporaryDirectory(prefix="rfort-playground-") as snapshot:
        compiler = None
        if args.local_compiler:
            compiler = str(Path(snapshot) / Path(args.local_compiler).name)
            shutil.copy2(args.local_compiler, compiler)
            print("Trusted local mode: submitted programs run with your account's privileges.", flush=True)
        gateway = Gateway(args.image, compiler, args.origin)
        server = ThreadingHTTPServer((args.bind, args.port), partial(Handler, directory=str(ROOT.parent / "playground")))
        server.gateway = gateway
        print(f"rfort playground: http://{args.bind}:{args.port}", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass
        finally:
            server.server_close()


if __name__ == "__main__":
    main()

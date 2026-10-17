"""Runs inside a disposable container; local mode is for trusted code only."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import threading
import time

from protocol import COMPILE_SECONDS, MAX_BODY, MAX_OUTPUT, RUN_SECONDS, validate


def terminate(process):
    if os.name == "nt":
        subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
    else:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


def execute(command, cwd, stdin="", seconds=RUN_SECONDS):
    process = subprocess.Popen(command, cwd=cwd, stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               start_new_session=os.name != "nt")
    # Like subprocess.run(timeout=...), start the budget after process creation.
    # Windows executable scanning can delay Popen before the child is available.
    start = time.monotonic()
    output = {"stdout": bytearray(), "stderr": bytearray()}
    overflow = threading.Event()
    lock = threading.Lock()
    count = 0

    def drain(stream, name):
        nonlocal count
        try:
            while chunk := stream.read(4096):
                with lock:
                    remaining = max(0, MAX_OUTPUT - count)
                    output[name].extend(chunk[:remaining])
                    count += len(chunk)
                    if count > MAX_OUTPUT:
                        overflow.set()
        finally:
            stream.close()

    def feed():
        try:
            process.stdin.write(stdin.encode("utf-8"))
            process.stdin.flush()
        except (BrokenPipeError, OSError):
            pass
        finally:
            process.stdin.close()

    threads = [threading.Thread(target=drain, args=(process.stdout, "stdout"), daemon=True),
               threading.Thread(target=drain, args=(process.stderr, "stderr"), daemon=True),
               threading.Thread(target=feed, daemon=True)]
    for thread in threads:
        thread.start()
    timed_out = False
    while process.poll() is None:
        timed_out = time.monotonic() - start > seconds
        if timed_out or overflow.is_set():
            if process.poll() is not None:
                timed_out = False
                break
            terminate(process)
            break
        time.sleep(0.02)
    process.wait(timeout=5)
    # Also stop descendants retaining pipes after their parent has exited.
    if os.name != "nt":
        terminate(process)
    for thread in threads:
        thread.join(timeout=1)
    return {**{key: bytes(data).decode("utf-8", errors="replace") for key, data in output.items()},
            "exit_code": process.returncode, "timed_out": timed_out,
            "output_limited": overflow.is_set(), "seconds": round(time.monotonic() - start, 3)}


def run_job(request, compiler):
    request = validate(request)
    with tempfile.TemporaryDirectory(prefix="rfort-") as directory:
        work = Path(directory)
        for file in request["files"]:
            (work / file["name"]).write_text(file["content"], encoding="utf-8")
        executable = work / ("program.exe" if os.name == "nt" else "program")
        command = [str(Path(compiler).resolve()), "-O", request["optimization"]]
        if request["warnings"]:
            command.append("-Wall")
        command.extend(file["name"] for file in request["files"])
        command.extend(["-o", str(executable)])
        build = execute(command, work, seconds=COMPILE_SECONDS)
        for key in ("stdout", "stderr"):
            build[key] = build[key].replace(str(work), ".")
        result = {"build": build, "run": None}
        if build["exit_code"] == 0 and not build["timed_out"] and not build["output_limited"] and request["action"] == "run":
            result["run"] = execute([str(executable)], work, request["stdin"])
        return result


def main():
    try:
        raw = sys.stdin.buffer.read(MAX_BODY + 1)
        if len(raw) > MAX_BODY:
            raise ValueError("Request too large.")
        result = run_job(json.loads(raw), os.environ.get("RFORT_COMPILER", "/opt/rfort/rfort"))
        print(json.dumps(result))
    except Exception as error:
        print(json.dumps({"error": str(error)}))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

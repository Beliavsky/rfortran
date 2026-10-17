import json
import os
from pathlib import Path
import sys
import tempfile
import threading
import unittest
from urllib.error import HTTPError
from urllib.request import Request, urlopen
from functools import partial
from http.server import ThreadingHTTPServer

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from protocol import validate
from server import Gateway, Handler, container_command
from worker import execute, run_job


def request(**updates):
    return {"files": [{"name": "main.f90", "content": "print *, 42\nend\n"}], **updates}


class ProtocolTests(unittest.TestCase):
    def test_allowed_request(self):
        self.assertEqual(validate(request())["optimization"], "0")

    def test_rejected_requests(self):
        invalid = [request(compiler="sh"), request(optimization="3"), request(warnings="yes"),
                   request(action="interpret"), request(stdin="x" * 32769), request(files=[]),
                   request(files=[{"name": "../main.f90", "content": "end"}]),
                   request(files=[{"name": "-c.f90", "content": "end"}]),
                   request(files=[{"name": "main.f90", "content": "x" * 131073}]),
                   request(files=[{"name": "main.f90", "content": ""}, {"name": "MAIN.F90", "content": ""}])]
        for item in invalid:
            with self.subTest(item=str(item)[:80]), self.assertRaises(ValueError):
                validate(item)

    def test_container_restrictions(self):
        command = container_command("image@sha256:abc", "job")
        for flag, value in [("--network", "none"), ("--memory", "256m"), ("--memory-swap", "256m"),
                            ("--pids-limit", "64"), ("--user", "65532:65532")]:
            self.assertEqual(command[command.index(flag) + 1], value)
        self.assertIn("--read-only", command)
        self.assertNotIn("--privileged", command)
        self.assertNotIn("-v", command)


class ProcessTests(unittest.TestCase):
    def test_timeout(self):
        with tempfile.TemporaryDirectory() as work:
            result = execute([sys.executable, "-I", "-S", "-c", "import time;time.sleep(10)"], work, seconds=0.15)
        self.assertTrue(result["timed_out"])
        self.assertLess(result["seconds"], 5)

    def test_output_limit(self):
        with tempfile.TemporaryDirectory() as work:
            result = execute([sys.executable, "-I", "-S", "-c", "import sys;sys.stdout.buffer.write(b'x'*200000);sys.stdout.flush()"], work, seconds=10)
        self.assertTrue(result["output_limited"])
        self.assertLessEqual(len(result["stdout"]), 65536)


class HTTPTests(unittest.TestCase):
    def setUp(self):
        # No external executable is run by these protocol tests.
        gateway = object.__new__(Gateway)
        gateway.compiler, gateway.origins = "local-test", set()
        gateway.metadata = {"compiler": "rfort"}
        gateway.slots = threading.BoundedSemaphore(2)
        gateway.allow = lambda address: True
        gateway.run = lambda payload: {"received": payload["action"]}
        directory = str(Path(__file__).resolve().parents[2] / "playground")
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), partial(Handler, directory=directory))
        self.server.gateway = gateway
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}"

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def test_health(self):
        with urlopen(self.url + "/api/health") as response:
            self.assertEqual(json.load(response)["compiler"], "rfort")

    def test_reject_cross_origin(self):
        req = Request(self.url + "/api/compile", data=json.dumps(request()).encode(),
                      headers={"Content-Type": "application/json", "Origin": "https://unrelated.example"})
        with self.assertRaises(HTTPError) as caught:
            urlopen(req)
        self.assertEqual(caught.exception.code, 403)

    def test_reject_rebinding_host(self):
        req = Request(self.url + "/api/health", headers={"Host": "unrelated.example"})
        with self.assertRaises(HTTPError) as caught:
            urlopen(req)
        self.assertEqual(caught.exception.code, 403)

    def test_compile_contract(self):
        req = Request(self.url + "/api/compile", data=json.dumps(request()).encode(), headers={"Content-Type": "application/json"})
        with urlopen(req) as response:
            self.assertEqual(json.load(response), {"received": "run"})

    def test_no_repository_files(self):
        for path in ("/../Cargo.toml", "/src/main.rs", "/tests/", "/%2e%2e/Cargo.toml"):
            with self.subTest(path=path), self.assertRaises(HTTPError) as caught:
                urlopen(self.url + path)
            self.assertEqual(caught.exception.code, 404)


@unittest.skipUnless(os.environ.get("RFORT_PLAYGROUND_COMPILER"), "Set RFORT_PLAYGROUND_COMPILER for native integration tests")
class NativeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.compiler = str(Path(os.environ["RFORT_PLAYGROUND_COMPILER"]).resolve())

    def test_hello(self):
        result = run_job(request(), self.compiler)
        self.assertEqual(result["build"]["exit_code"], 0, result)
        self.assertEqual(result["run"]["stdout"].strip(), "42", result)
        self.assertFalse(result["run"]["timed_out"], result)

    def test_compile_only(self):
        result = run_job(request(action="compile"), self.compiler)
        self.assertEqual(result["build"]["exit_code"], 0, result)
        self.assertIsNone(result["run"])

    def test_compile_error_does_not_run(self):
        result = run_job(request(files=[{"name":"main.f90", "content":"not valid Fortran !!!\n"}]), self.compiler)
        self.assertNotEqual(result["build"]["exit_code"], 0)
        self.assertIsNone(result["run"])

    def test_input(self):
        result = run_job(request(stdin="7\n", files=[{"name":"main.f90", "content":"integer :: n\nread *, n\nprint *, n*n\nend\n"}]), self.compiler)
        self.assertEqual(result["build"]["exit_code"], 0, result)
        self.assertEqual(result["run"]["stdout"].strip(), "49", result)

    def test_modules_and_optimization(self):
        result = run_job(request(optimization="2", warnings=True, files=[
            {"name":"numbers.f90", "content":"module numbers\ninteger, parameter :: n=17\nend module numbers\n"},
            {"name":"main.f90", "content":"use numbers\nprint *, n\nend\n"}]), self.compiler)
        self.assertEqual(result["build"]["exit_code"], 0, result)
        self.assertEqual(result["run"]["stdout"].strip(), "17", result)
        self.assertFalse(result["run"]["timed_out"], result)


if __name__ == "__main__":
    unittest.main()

"""Runner behavior without building or changing the compiler."""
from pathlib import Path
import tempfile
import sys
import unittest
from unittest.mock import patch

from run_m_intrinsics import discover, failed, invoke, probe, runtime_exclusion, summarize


class MIntrinsicsRunner(unittest.TestCase):
    def test_timeout_terminates_process_and_preserves_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            result=invoke([sys.executable,"-S","-c","import time; print('started', flush=True); time.sleep(30)"],
                          tmp,2)
        self.assertTrue(result.get("timeout"))
        self.assertNotEqual(result["returncode"],0)
        self.assertIn("started",result["stdout"])

    def test_discovery_filters_and_extensions(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "example").mkdir()
            for name in ["demo_abs.f90", "demo_scale.F90", "demo_bad.txt", "other.f90"]:
                (root / "example" / name).write_text("end\n")
            names = [p.name for p in discover(root, ["demo_*"], ["*abs*"])]
            self.assertEqual(names, ["demo_scale.F90"])

    def test_runtime_screen_preserves_quoted_exclamation(self):
        self.assertIsNone(runtime_exclusion("print *, 'call random_number(x)!'\nend"))
        self.assertIn("randomness", runtime_exclusion("print *, 'hello!'; call random_number(x)"))
        for source in ["read(*,*)x", "open(10,file='input')", "call execute_command_line('dir')",
                       "print *,compiler_version()", "use ieee_arithmetic", "interface"]:
            with self.subTest(source=source):
                self.assertIsNotNone(runtime_exclusion(source))

    def execute(self, responses, execute=False, source="print *, 1\nend\n", reference="gfortran"):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "demo_value.f90"
            path.write_text(source)
            original = path.read_bytes()
            with patch("run_m_intrinsics.invoke", side_effect=responses) as mocked:
                result = probe(path, "rfort", reference, ["0", "2"], 3, execute, [])
            self.assertEqual(path.read_bytes(), original)
            return result, mocked.call_args_list

    def test_compile_only_does_not_execute_programs(self):
        ok = dict(returncode=0, stdout="", stderr="")
        result, calls = self.execute([ok, ok, ok])
        self.assertEqual(result["status"], "compiled")
        self.assertEqual(len(calls), 3)
        self.assertEqual(calls[1].args[0][1:3], ["-O", "0"])
        self.assertEqual(calls[2].args[0][1:3], ["-O", "2"])
        self.assertEqual(calls[0].args[0][2].name, "demo_value.f90")
        self.assertIn("sha256", result)

    def test_reference_failure_does_not_suppress_rfort_builds(self):
        ok = dict(returncode=0, stdout="", stderr="")
        result, calls = self.execute([dict(ok, returncode=1), ok, ok], execute=True)
        self.assertEqual(result["reference"]["status"], "reference_compile_error")
        self.assertEqual(result["status"], "compiled_no_reference_oracle")
        self.assertEqual(len(calls), 3)

    def test_runtime_compares_both_optimizations(self):
        ok = dict(returncode=0, stdout="", stderr="")
        output = dict(ok, stdout="1 T 0.5\n")
        result, calls = self.execute([ok, output, output, ok, output, ok, output], execute=True)
        self.assertEqual(result["status"], "pass")
        self.assertEqual(len(calls), 7)
        self.assertNotEqual(calls[1].args[1], calls[2].args[1])

    def test_nondeterministic_reference_does_not_get_compared(self):
        ok = dict(returncode=0, stdout="", stderr="")
        result, calls = self.execute([ok, dict(ok, stdout="1"), dict(ok, stdout="2"), ok, ok], execute=True)
        self.assertEqual(result["reference"]["status"], "reference_nondeterministic")
        self.assertEqual(result["status"], "compiled_no_reference_oracle")
        self.assertEqual(len(calls), 5)

    def test_random_demo_is_compiled_but_not_run(self):
        ok = dict(returncode=0, stdout="", stderr="")
        result, calls = self.execute([ok, ok, ok], execute=True, source="call random_number(x)\nend")
        self.assertEqual(result["status"], "compiled_run_skipped")
        self.assertEqual(len(calls), 3)

    def test_wrong_output_and_integer_values_are_not_hidden(self):
        ok = dict(returncode=0, stdout="", stderr="")
        output = dict(ok, stdout="1000000000000000000\n")
        result, _ = self.execute([ok, output, output, ok, output,
                                  ok, dict(ok, stdout="1000000000000000001\n")], execute=True)
        self.assertEqual(result["results"]["O2"]["status"], "wrong_output")
        self.assertEqual(result["status"], "mixed")

    def test_crashes_timeouts_and_errors_are_separate(self):
        for result, expected in [({"timeout": True}, "compile_timeout"),
                                 ({"launch_error": True}, "compile_launch_error"),
                                 ({"returncode": -1073741819}, "compile_crash"),
                                 ({"returncode": 101}, "compile_crash"),
                                 ({"returncode": 1}, "compile_error")]:
            self.assertEqual(failed(result, "compile"), expected)

    def test_summary_does_not_call_compile_success_a_pass(self):
        report = dict(optimization_levels=["0"], cases=[dict(status="compiled",
                      runtime_exclusion=None, results={"O0": {"status": "compiled"}})])
        summarize(report)
        self.assertEqual(report["summary"], {"compiled": 1})
        self.assertNotIn("pass", report["optimization_summary"]["O0"])


if __name__ == "__main__":
    unittest.main()

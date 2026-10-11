"""Keep the compatibility comparator from hiding integer/logical failures."""
import unittest
import json
from pathlib import Path
import tempfile
from unittest.mock import patch

from run_ofort_compat import diagnostic_group, discover, equivalent, failure, probe, source_code, summarize

class CompatibilityReporting(unittest.TestCase):
    def test_numeric_and_logical_comparison(self):
        self.assertTrue(equivalent("1 1.25 .TRUE.\n.FALSE.", " 1 1.2500000 T F"))
        self.assertTrue(equivalent("2.5D+00", "2.5000000E+00"))
        self.assertFalse(equivalent("9007199254740992", "9007199254740993"))
        self.assertFalse(equivalent("T", ".FALSE."))
        self.assertFalse(equivalent("1 2", "1"))
        self.assertFalse(equivalent("1.1", "1.2"))
        self.assertFalse(equivalent("NaN", "1.0"))

    def test_failures_are_not_output_mismatches(self):
        self.assertEqual(failure({"returncode":101},"compile"),"compile_crash")
        self.assertEqual(failure({"returncode":-1073741819},"run"),"run_crash")
        self.assertEqual(failure({"returncode":3, "stderr":"Program received signal SIGSEGV"},"reference_run"),"reference_run_crash")
        self.assertEqual(failure({"returncode":1},"compile"),"compile_error")
        self.assertEqual(failure({"timeout":True},"run"),"run_timeout")
        self.assertEqual(failure({"launch_error":True},"run"),"run_launch_error")
        self.assertIsNone(failure({"returncode":0},"compile"))

    def test_discovery_records_exclusions_and_keeps_pending_cases(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            directory = root / "tests/cases"
            directory.mkdir(parents=True)
            sources = {
                "xplain.f90": "program p\nprint *, 1\nend program p",
                "xvalid_pending.f90": "program p\nprint *, 2\nend program p",
                "xinput.f90": "read (*,*) i",
                "xrandom.f90": "call random_number(x)",
                "xexternal.f90": "use other_module",
                "xlocal.f90": "module m\nend module m\nprogram p\nuse m\nend program p",
                "xenv.F90": "use, intrinsic :: iso_fortran_env",
                "xquoted.f90": "print *, 'call random_number(x) ! use missing'\n! read(*,*) x",
                "xpartial_init_unset_pending.f90": "print *, x",
                "xalloc_huge_stat.f90": "allocate(x(10000000000_8))",
                "temp_try.f90": "print *, 1",
                "xunknown.f90": "print *, 1",
            }
            for name, contents in sources.items():
                source = directory / name
                source.write_text(contents, encoding="utf-8")
                if name not in {"xvalid_pending.f90", "xunknown.f90"}:
                    source.with_suffix(".out").write_text("ignored oracle", encoding="utf-8")
            cases, inventory = discover(root)
            self.assertEqual(cases, ["xenv.F90", "xlocal.f90", "xplain.f90", "xquoted.f90", "xvalid_pending.f90"])
            self.assertEqual(inventory["source_count"], 12)
            self.assertEqual(inventory["candidate_count"], 5)
            reasons = {item["source"]: item["reason"] for item in inventory["excluded"]}
            self.assertIn("standard-input", reasons["xinput.f90"])
            self.assertIn("companion module", reasons["xexternal.f90"])
            self.assertIn("40 GB", reasons["xalloc_huge_stat.f90"])

    def test_source_scan_preserves_code_after_quoted_exclamation(self):
        code = source_code("s='don''t!'; call random_number(x) ! comment\nuse m")
        self.assertIn("call random_number(x)", code)
        self.assertIn("use m", code)
        self.assertNotIn("comment", code)

    def execute_probe(self, responses, repeat=True):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            directory = root / "tests/cases"
            directory.mkdir(parents=True)
            (directory / "xplain.f90").write_text("print *, 1", encoding="utf-8")
            with patch("run_ofort_compat.run", side_effect=responses) as mocked:
                result = probe("xplain.f90", root, "rfort", "gfortran", 5, repeat)
            return result, mocked.call_args_list

    def test_probe_checks_both_optimizations_and_reference_in_fresh_directory(self):
        ok = dict(returncode=0, stdout="", stderr="")
        expected = dict(returncode=0, stdout="1 T\n", stderr="")
        actual = dict(returncode=0, stdout=" 1 .TRUE.\n", stderr="")
        result, calls = self.execute_probe([ok, expected, expected, ok, actual, ok, actual])
        self.assertEqual(result["status"], "pass")
        self.assertEqual(list(result["results"]), ["O0", "O2"])
        self.assertNotEqual(calls[1].args[1], calls[2].args[1])
        self.assertEqual(calls[3].args[0][1:3], ["-O", "0"])
        self.assertEqual(calls[5].args[0][1:3], ["-O", "2"])
        self.assertIn("sha256", result)

    def test_nondeterministic_reference_is_not_a_compiler_failure(self):
        ok = dict(returncode=0, stdout="", stderr="")
        result, calls = self.execute_probe([ok, dict(ok, stdout="1"), dict(ok, stdout="2")])
        self.assertEqual(result["status"], "reference_nondeterministic")
        self.assertEqual(result["results"], {})
        self.assertEqual(len(calls), 3)

    def test_reference_compile_failure_is_separate(self):
        result, calls = self.execute_probe([dict(returncode=1, stdout="", stderr="invalid source")])
        self.assertEqual(result["status"], "reference_compile_error")
        self.assertEqual(result["results"], {})
        self.assertEqual(len(calls), 1)

    def test_missing_source_does_not_invoke_compilers(self):
        with tempfile.TemporaryDirectory() as tmp, patch("run_ofort_compat.run") as mocked:
            result = probe("missing.f90", Path(tmp), "rfort", "gfortran", 5)
        self.assertEqual(result["status"], "missing_source")
        mocked.assert_not_called()

    def test_diagnostic_groups_keep_crashes_separate_from_language_errors(self):
        for message, expected in [
            ("Error: parse failed", "parse error"),
            ("Error: read of uninitialized variable `x`", "initialization error"),
            ("Semantic: undefined function `bessel_j0`", "name resolution error"),
            ("Error: assumed rank is not yet supported", "unsupported feature"),
            ("Error: array shape mismatch", "other compile error"),
        ]:
            with self.subTest(message=message):
                self.assertEqual(diagnostic_group({"status": "compile_error", "build": {"stderr": message}}), expected)
        self.assertEqual(diagnostic_group({"status": "compile_crash"}), "compile_crash")

    def test_probe_distinguishes_wrong_output_stderr_and_compile_crash(self):
        ok = dict(returncode=0, stdout="", stderr="")
        result, _ = self.execute_probe([ok, dict(ok, stdout="1"), ok, dict(ok, stdout="2"),
                                       dict(returncode=101, stdout="", stderr="panic")], repeat=False)
        self.assertEqual(result["results"]["O0"]["status"], "wrong_output")
        self.assertEqual(result["results"]["O2"]["status"], "compile_crash")
        result, _ = self.execute_probe([ok, dict(ok, stdout="1"), ok, dict(ok, stdout="1", stderr="warning"),
                                       ok, dict(ok, stdout="1")], repeat=False)
        self.assertEqual(result["results"]["O0"]["status"], "unexpected_stderr")
        self.assertEqual(result["results"]["O2"]["status"], "pass")

    def test_summary_counts_each_failed_case_once(self):
        entry = {"status": "compile_error", "diagnostic_group": "parse error"}
        report = {"cases": [
            {"status": "fail", "feature": "allocation", "results": {"O0": entry, "O2": entry}},
            {"status": "reference_compile_error", "feature": "numeric", "results": {}},
            {"status": "pass", "feature": "allocation", "results": {
                "O0": {"status": "pass"}, "O2": {"status": "pass"}}},
        ]}
        summarize(report)
        self.assertEqual(report["failure_groups"], {"parse error": 1})
        self.assertEqual(report["optimization_summary"]["O0"], {"compile_error": 1, "pass": 1})
        self.assertEqual(report["feature_summary"]["allocation"], {"fail": 1, "pass": 1})

    def test_recorded_baseline_accounts_for_each_source_once(self):
        snapshot = json.loads((Path(__file__).parent / "ofort/expanded-baseline.json").read_text(encoding="utf-8"))
        candidates = [name for names in snapshot["case_groups"].values() for name in names]
        excluded = [name for names in snapshot["excluded_groups"].values() for name in names]
        self.assertEqual(len(set(candidates)), snapshot["candidate_count"])
        self.assertEqual(len(candidates), snapshot["candidate_count"])
        self.assertEqual(len(set(excluded)), snapshot["excluded_count"])
        self.assertFalse(set(candidates) & set(excluded))
        self.assertEqual(len(candidates) + len(excluded), snapshot["source_count"])
        self.assertEqual(sum(snapshot["summary"].values()), len(candidates))
        for counts in snapshot["optimization_summary"].values():
            self.assertEqual(sum(counts.values()), snapshot["reference_eligible_count"])

if __name__=="__main__":
    unittest.main()

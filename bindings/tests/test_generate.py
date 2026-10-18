import copy
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from generate import SpecError, generate, validate


class GeneratorTests(unittest.TestCase):
    def setUp(self):
        self.spec = json.loads((ROOT / "examples" / "demo.json").read_text())

    def test_value_pointer_and_array_abi(self):
        result = generate(self.spec)
        self.assertIn("integer(bg_c_size_t), value, intent(in)", result["bindings.f90"])
        self.assertIn("x(bg_n_x)", result["bindings.f90"])
        self.assertIn("x: *const f64", result["bindings.rs"])
        self.assertIn("x: *mut f64", result["bindings.rs"])
        self.assertIn("code: *mut i32", result["bindings.rs"])
        self.assertIn("pub unsafe fn affine(x: &mut [f64], a: f64, b: f64)", result["bindings.rs"])

    def test_unknown_or_unsupported_attributes_are_rejected(self):
        for field, value in [("optional", True), ("pointer", True), ("allocatable", True), ("shape", [3])]:
            with self.subTest(field=field):
                spec = copy.deepcopy(self.spec)
                spec["procedures"][0]["args"][0][field] = value
                with self.assertRaisesRegex(SpecError, "unsupported fields"):
                    generate(spec)

    def test_kinds_and_ranks_are_not_guessed(self):
        for field, value in [("type", "real"), ("type", "complex"), ("rank", 2), ("rank", True)]:
            with self.subTest(field=field, value=value):
                spec = copy.deepcopy(self.spec)
                spec["procedures"][0]["args"][0][field] = value
                with self.assertRaises(SpecError):
                    generate(spec)

    def test_name_collisions_and_injection_are_rejected(self):
        for name in ("../bad", "x); call evil()", "bg_result", "type"):
            with self.subTest(name=name):
                spec = copy.deepcopy(self.spec)
                spec["procedures"][0]["args"][0]["name"] = name
                with self.assertRaises(SpecError):
                    generate(spec)
        self.spec["procedures"].append(copy.deepcopy(self.spec["procedures"][0]))
        with self.assertRaisesRegex(SpecError, "duplicate Rust name"):
            generate(self.spec)

    def test_length_contract_requires_arrays(self):
        self.spec["procedures"][0]["same_length"] = [["x", "missing"]]
        with self.assertRaisesRegex(SpecError, "rank-one"):
            generate(self.spec)

    def test_rust_alias_preserves_original_fortran_name(self):
        self.spec["procedures"][0]["rust_name"] = "double_value"
        result = generate(self.spec)
        self.assertIn("bg_target => twice", result["bindings.f90"])
        self.assertIn("pub unsafe fn double_value", result["bindings.rs"])

    def test_zero_argument_procedures(self):
        spec = {"library":"demo", "procedures":[{"module":"m", "name":"answer", "kind":"function", "result":"i32", "args":[]}]}
        result = generate(spec)
        self.assertIn("function bg_wrap_0() result(bg_result)", result["bindings.f90"])
        self.assertIn("bg_result = bg_target()", result["bindings.f90"])
        self.assertIn("pub unsafe fn answer() -> i32", result["bindings.rs"])

    def test_long_identifiers_are_rejected_if_output_would_be_invalid(self):
        self.spec["procedures"][1]["args"][0]["name"] = "x" * 64
        with self.assertRaises(SpecError):
            generate(self.spec)


@unittest.skipUnless(shutil.which("gfortran") and shutil.which("rustc"), "gfortran and rustc required")
class IntegrationTests(unittest.TestCase):
    def build_example(self, example):
        build_root = ROOT.parent / "target"
        build_root.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="bindings-test-", dir=build_root) as work:
            result = subprocess.run([sys.executable, "-S", str(ROOT / "run_demo.py"), "--example", example, "--out-dir", work],
                                    capture_output=True, text=True, timeout=180)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            return result.stdout

    def test_rust_calls_fortran_shared_library(self):
        self.assertIn("Rust called Fortran:", self.build_example("demo"))

    @unittest.skipUnless((ROOT.parent / "rnorm.f90").exists(), "Existing root rnorm.f90 example required")
    def test_rust_calls_existing_rnorm(self):
        self.assertIn("10000 finite normal samples", self.build_example("rnorm"))


if __name__ == "__main__":
    unittest.main()

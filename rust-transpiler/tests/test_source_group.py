from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from compiler_frontend import FrontendError, Unsupported, find_compiler
from source_group import SourceGroup
from transpile import transpile_files

try:
    find_compiler()
    HAS_FRONTEND = True
except FrontendError:
    HAS_FRONTEND = False


class SourceMapTests(unittest.TestCase):
    def test_file_boundaries_and_unicode(self):
        group = SourceGroup([("mod.f90","! μ\nmodule m\nend module m"),("main.f90","implicit none\nend\n")])
        self.assertEqual(group.starts,[1,4])
        self.assertEqual(group.locate(3),("mod.f90",3))
        self.assertEqual(group.locate(4),("main.f90",1))
        self.assertIn("end module m\nimplicit none",group.source)
        diagnostics = group.diagnostics(" ┌─ joined.f90:5:4\n5 │ end\n","joined.f90")
        self.assertIn("main.f90:2:4",diagnostics)
        self.assertIn("2 │ end",diagnostics)

    def test_empty_and_duplicate_groups_fail(self):
        with self.assertRaisesRegex(FrontendError,"at least one"):
            SourceGroup([])
        with self.assertRaisesRegex(FrontendError,"duplicate source"):
            SourceGroup.read(["same.f90","same.f90"])


@unittest.skipUnless(HAS_FRONTEND,"Existing rfort executable required")
class GroupFrontendTests(unittest.TestCase):
    def setUp(self):
        parent = ROOT.parent / "target"
        parent.mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="source-group-test-",dir=parent)
        self.addCleanup(self.temp.cleanup)
        self.work = Path(self.temp.name)
        example = ROOT / "examples/modules"
        self.paths = []
        for name in ("base.f90","derived.f90","main.f90"):
            path = self.work / name
            path.write_text((example / name).read_text(),encoding="utf-8")
            self.paths.append(path)

    def test_group_uses_existing_frontend_and_keeps_source_comments(self):
        code = transpile_files(self.paths,source_comments=True)
        self.assertIn("mod base_math",code)
        self.assertIn("mod derived_math",code)
        self.assertIn("// Fortran "+str(self.paths[-1])+":4",code)
        self.assertIn("// Fortran "+str(self.paths[0])+":8",code)

    def test_backend_failure_refers_to_original_driver_line(self):
        main = self.paths[-1]
        main.write_text("program p\nuse derived_math\nimplicit none\ncharacter :: text\nend\n")
        with self.assertRaises(Unsupported) as caught:
            transpile_files(self.paths)
        self.assertEqual(caught.exception.filename,str(main))
        self.assertEqual(caught.exception.line,4)

    def test_driver_without_implicit_none_is_accepted(self):
        main = self.paths[-1]
        main.write_text("program p\nprint *,1\nend\n")
        code = transpile_files(self.paths,source_comments=True)
        self.assertIn("// Fortran "+str(main)+":2",code)

    def test_parse_failure_refers_to_original_file(self):
        main = self.paths[-1]
        main.write_text("program p\nimplicit none\nprint *, )\nend\n")
        with self.assertRaises(FrontendError) as caught:
            transpile_files(self.paths)
        self.assertIn(str(main)+":3:",str(caught.exception))
        self.assertNotIn("part-2.f90",str(caught.exception))

    def test_locationless_semantic_failure_preserves_compiler_diagnostic(self):
        derived = self.paths[1]
        source = derived.read_text().replace("j = multiply(i)+factor","print *,i\nj = multiply(i)+factor")
        derived.write_text(source)
        with self.assertRaises(FrontendError) as caught:
            transpile_files(self.paths)
        self.assertIn("PURE procedure cannot perform external I/O",str(caught.exception))

    def test_located_semantic_failure_refers_to_original_driver_line(self):
        main = self.paths[-1]
        main.write_text("program p\nuse derived_math\nimplicit none\nreal :: x(2)\nx=1.0\nprint *,sum(x(1:3))\nend\n")
        with self.assertRaises(FrontendError) as caught:
            transpile_files(self.paths)
        self.assertIn(str(main)+":6:",str(caught.exception))
        self.assertIn("6 │ print *,sum",str(caught.exception))

    def test_dependencies_must_precede_consumers(self):
        with self.assertRaisesRegex(Unsupported,"must precede") as caught:
            transpile_files([self.paths[1],self.paths[0],self.paths[2]])
        self.assertEqual(caught.exception.filename,str(self.paths[1]))
        self.assertEqual(caught.exception.line,2)

    def test_main_program_cannot_precede_module_inputs(self):
        with self.assertRaisesRegex(FrontendError,"only complete module definitions"):
            transpile_files([self.paths[-1],self.paths[0]])

    @unittest.skipUnless(shutil.which("rustc"),"rustc required")
    def test_cli_runs_and_defaults_to_last_source_name(self):
        result = subprocess.run([sys.executable,str(ROOT / "transpile.py"),*map(str,self.paths),"--run"],cwd=self.work,capture_output=True,text=True,timeout=120)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(result.stdout.split(),["8"])
        self.assertTrue((self.work / "target/rust-transpiler/main.rs").is_file())
        if shutil.which("gfortran"):
            exe = self.work / ("reference.exe" if sys.platform=="win32" else "reference.bin")
            compiled = subprocess.run(["gfortran",*map(str,self.paths),"-o",str(exe)],cwd=self.work,capture_output=True,text=True,timeout=120)
            self.assertEqual(compiled.returncode,0,compiled.stderr)
            reference = subprocess.run([str(exe)],cwd=self.work,capture_output=True,text=True,timeout=10)
            self.assertEqual(reference.returncode,0,reference.stderr)
            self.assertEqual(result.stdout.split(),reference.stdout.split())

    @unittest.skipUnless(shutil.which("rustc"),"rustc required")
    def test_runtime_array_error_identifies_original_driver_line(self):
        main = self.paths[-1]
        main.write_text("program p\nuse derived_math\nimplicit none\nreal :: x(2)\ninteger :: i\nx=1.0\ni=3\nprint *,x(i)\nend\n")
        result = subprocess.run([sys.executable,str(ROOT / "transpile.py"),*map(str,self.paths),"--run"],cwd=self.work,capture_output=True,text=True,timeout=120)
        self.assertNotEqual(result.returncode,0)
        self.assertIn(f"Fortran {main}:8: array subscript 3",result.stderr)


if __name__ == "__main__":
    unittest.main()

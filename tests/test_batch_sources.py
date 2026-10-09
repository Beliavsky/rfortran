"""Windows batch integration checks; requires a built f90c and the batch's linker setup.

Run: python tests/test_batch_sources.py
"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
BATCH = ROOT / "f90c.bat"
FIXTURES = ROOT / "tests" / "multifile"


@unittest.skipUnless(os.name == "nt", "Windows batch wrapper")
class BatchSources(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="f90c sources (A&B)! ")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def batch(self, *sources):
        # Explicit cmd quoting also handles shell metacharacters in path names.
        arguments = " ".join('"' + str(p) + '"' for p in (BATCH, *sources))
        return subprocess.run(
            'cmd.exe /d /v:off /s /c "' + arguments + '"',
            cwd=self.directory, capture_output=True, text=True, timeout=90,
        )

    def sources(self):
        paths = []
        for name in ("m1", "m2", "main"):
            path = self.directory / (name + " source.f90")
            # Exercise BOM removal and a trailing comment with no final newline.
            path.write_text(
                (FIXTURES / (name + ".f90")).read_text(encoding="utf-8")
                + "\n! trailing comment", encoding="utf-8-sig",
            )
            paths.append(path)
        return paths

    def check_build(self, sources, expected="42"):
        result = self.batch(*sources)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(result.stdout + result.stderr, "")
        exe = sources[-1].with_suffix(".exe")
        self.assertTrue(exe.exists())
        result = subprocess.run([str(exe)], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), expected)
        self.assertEqual(result.stderr, "")

    def test_module_dependency_chain_and_repeat_build(self):
        sources = self.sources()
        self.check_build(sources)
        combined = sources[-1].with_suffix(".f90c-sources.f90")
        self.assertTrue(combined.exists())
        text = combined.read_text(encoding="utf-8")
        self.assertNotIn("\ufeff", text)
        self.assertLess(text.index("module m1"), text.index("module m2"))
        self.assertLess(text.index("module m2"), text.index("program main"))
        self.assertFalse(sources[0].with_suffix(".exe").exists())
        self.check_build(sources)

    def test_more_than_nine_inputs(self):
        sources = self.sources()
        extra = []
        for i in range(8):
            path = self.directory / f"extra {i}.f90"
            path.write_text("! empty source", encoding="utf-8")
            extra.append(path)
        self.check_build([*extra, *sources])

    def test_single_source(self):
        source = self.directory / "single source.f90"
        source.write_text("print *, 42\nend\n", encoding="utf-8")
        self.check_build([source])
        self.assertFalse(source.with_suffix(".f90c-sources.f90").exists())

    def test_kind_module(self):
        source = (ROOT / "tests" / "58_selected_kinds.f90").read_text(encoding="utf-8")
        module, main = source.split("end module kind_mod", 1)
        kind = self.directory / "kind.f90"
        xkind = self.directory / "xkind.f90"
        kind.write_text(module + "end module kind_mod\n", encoding="utf-8")
        xkind.write_text(main, encoding="utf-8")
        self.check_build([kind, xkind], "8 4 8 4 8 -999\n -999.0")

    def test_missing_source_preserves_existing_outputs(self):
        sources = self.sources()
        exe = sources[-1].with_suffix(".exe")
        exe.write_bytes(b"previous executable")
        result = self.batch(sources[0], self.directory / "missing.f90", sources[-1])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Source file not found", result.stderr)
        self.assertEqual(exe.read_bytes(), b"previous executable")
        self.assertFalse(sources[-1].with_suffix(".f90c-sources.f90").exists())

    def test_compile_failure_keeps_combined_source(self):
        sources = self.sources()
        sources[-1].write_text("program main\ninteger :: =\nend program main\n", encoding="utf-8")
        result = self.batch(*sources)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("main source.f90c-sources.f90", result.stderr)
        self.assertTrue(sources[-1].with_suffix(".f90c-sources.f90").exists())
        self.assertFalse(sources[-1].with_suffix(".exe").exists())

    def test_refuses_to_overwrite_user_source(self):
        sources = self.sources()
        combined = sources[-1].with_suffix(".f90c-sources.f90")
        combined.write_text("! handwritten source\n", encoding="utf-8")
        result = self.batch(*sources)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Refusing to overwrite", result.stderr)
        self.assertEqual(combined.read_text(encoding="utf-8"), "! handwritten source\n")


if __name__ == "__main__":
    unittest.main(verbosity=2)

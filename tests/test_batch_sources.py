"""Windows batch integration checks; requires a built rfort and the batch's linker setup.

Run: python tests/test_batch_sources.py
"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
BATCH = ROOT / "rfort.bat"
FIXTURES = ROOT / "tests" / "multifile"


@unittest.skipUnless(os.name == "nt", "Windows batch wrapper")
class BatchSources(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="f90c sources (A&B)! ")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def batch(self, *sources, wrapper=BATCH):
        # Explicit cmd quoting also handles shell metacharacters in path names.
        arguments = " ".join('"' + str(p) + '"' for p in (wrapper, *sources))
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

    def test_legacy_wrapper(self):
        sources = self.sources()
        result = self.batch(*sources, wrapper=ROOT / "f90c.bat")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(result.stdout + result.stderr, "")
        result = subprocess.run([sources[-1].with_suffix(".exe")], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "42")
        result = self.batch(self.directory / "missing.f90", wrapper=ROOT / "f90c.bat")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Source file not found", result.stderr)

    def test_defined_operators(self):
        source = self.directory / "defined operators.f90"
        source.write_text((ROOT / "tests/76_defined_operators.f90").read_text())
        self.check_build([source], "14\n36\n18\n6\n  3.5\nF\nmatched\n10\n12")

    def test_multi_forall(self):
        import json
        fixture = ROOT / "tests/80_multi_forall.f90"
        source = self.directory / "multi forall.f90"
        source.write_text(fixture.read_text(encoding="utf-8"), encoding="utf-8")
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][fixture.stem]["program_stdout"]
        self.check_build([source], expected.strip())

    def test_else_if(self):
        source = self.directory / "else if.f90"
        source.write_text((ROOT / "tests/79_else_if.f90").read_text(encoding="utf-8"), encoding="utf-8")
        self.check_build([source], "7\n10\n20\n30\n40\n99\n50\n60")

    def test_character_lengths(self):
        import json
        fixture = ROOT / "tests/78_character_lengths.f90"
        source = self.directory / "character lengths.f90"
        source.write_text(fixture.read_text(encoding="utf-8"), encoding="utf-8")
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][fixture.stem]["program_stdout"]
        self.check_build([source], expected.strip())

    def test_file_positioning(self):
        import json
        source = self.directory / "file positioning.f90"
        fixture = ROOT / "tests/77_file_positioning.f90"
        source.write_text(fixture.read_text().replace("'tests/io/file_records.txt'", "'" + (ROOT / "tests/io/file_records.txt").as_posix() + "'"))
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][fixture.stem]["program_stdout"]
        self.check_build([source], expected.strip())

    def test_compiler_command_name(self):
        from test_record_read import COMPILER
        for argument in ("--help", "--version"):
            result = subprocess.run([COMPILER, argument], capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("rfort", result.stdout)

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

    def test_write_stdout_and_bare_stop(self):
        import json
        source = self.directory / "write output.f90"
        source.write_text((ROOT / "tests/74_write_stdout.f90").read_text(), encoding="utf-8")
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"]["74_write_stdout"]["program_stdout"]
        self.check_build([source], expected.strip())

    def test_internal_integer_input(self):
        import json
        source = self.directory / "internal input.f90"
        source.write_text((ROOT / "tests/73_internal_integer_read.f90").read_text(), encoding="utf-8")
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"]["73_internal_integer_read"]["program_stdout"]
        self.check_build([source], expected.strip())

    def test_formatted_record_input(self):
        source = self.directory / "record input.f90"
        source.write_text("character(len=8) :: text\nread(*,'(a)') text\nprint '(a)', text\nend\n", encoding="utf-8")
        result = self.batch(source)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(result.stdout + result.stderr, "")
        result = subprocess.run([str(source.with_suffix(".exe"))], input="  abc\n",
                                capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "  abc   \n")
        self.assertEqual(result.stderr, "")

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

    def test_use_only_module_chain(self):
        source = (ROOT / "tests" / "60_use_only.f90").read_text(encoding="utf-8")
        base, rest = source.split("end module selected_base", 1)
        middle, main = rest.split("end module selected_middle", 1)
        paths = [self.directory / name for name in ("base.f90", "middle.f90", "main.f90")]
        for path, text in zip(paths, [base + "end module selected_base\n", middle + "end module selected_middle\n", main]):
            path.write_text(text, encoding="utf-8")
        self.check_build(paths, "7 22 100 1.5")

    def test_generic_module_chain(self):
        source = (ROOT / "tests" / "63_generic_interfaces.f90").read_text(encoding="utf-8")
        base, rest = source.split("end module generic_values", 1)
        bridge, main = rest.split("end module generic_bridge", 1)
        paths = [self.directory / name for name in ("values.f90", "bridge.f90", "main.f90")]
        for path, text in zip(paths, [base + "end module generic_values\n", bridge + "end module generic_bridge\n", main]):
            path.write_text(text, encoding="utf-8")
        self.check_build(paths, "11 21.0 31.0\n 22 7\n 42 43\n 4.0 5.0 5.0\n 25.0")

    def test_named_integer_kinds(self):
        source = (ROOT / "tests" / "65_named_integer_kinds.f90").read_text(encoding="utf-8")
        module, main = source.split("end module integer_kind_values", 1)
        kind = self.directory / "integer kinds.f90"
        program = self.directory / "integer main.f90"
        kind.write_text(module + "end module integer_kind_values\n", encoding="utf-8")
        program.write_text(main, encoding="utf-8")
        self.check_build([kind, program], "100 30000 1000000000\n 9000000000 9000000000 18000000000 123\n 4 8 8 17\n 11 11\n 9000000001")

    def test_allocatable_matrices(self):
        source = (ROOT / "tests" / "66_allocatable_matrices.f90").read_text(encoding="utf-8")
        module, main = source.split("end module matrix_operations", 1)
        library = self.directory / "matrix library.f90"
        program = self.directory / "matrix main.f90"
        library.write_text(module + "end module matrix_operations\n", encoding="utf-8")
        program.write_text(main, encoding="utf-8")
        self.check_build([library, program], "6 2 3\n 11 12 21 22 31 32\n 12 13 22 23 32 33\n 32\n 31\n 8 7 56\n 1.5 2.5 1.5 1.5 7.0\n 2")

    def test_character_parameters(self):
        source = (ROOT / "tests" / "68_character_parameters.f90").read_text(encoding="utf-8")
        module, main = source.split("end module character_values", 1)
        library = self.directory / "character library.f90"
        program = self.directory / "character main.f90"
        library.write_text(module + "end module character_values\n", encoding="utf-8")
        program.write_text(main, encoding="utf-8")
        self.check_build([library, program], "[util::ready]\n [xy   ]\n [abcde]\n Ax\n first\nsecond\n util::child!")

    def test_matrix_sections(self):
        source = (ROOT / "tests" / "69_matrix_sections.f90").read_text(encoding="utf-8")
        program = self.directory / "matrix sections.f90"
        program.write_text(source, encoding="utf-8")
        self.check_build([program], "1 1 1 1\n 21 22 23\n 23 22 21 13 12 11\n 32 34 36\n 68\n 0\n 1 1 1 1\n 5 5 5 9\n 13 12 11 23 22 21")

    def test_elemental_functions(self):
        source = (ROOT / "tests" / "70_elemental_functions.f90").read_text(encoding="utf-8")
        module, main = source.split("end module elemental_math", 1)
        library = self.directory / "elemental library.f90"
        program = self.directory / "elemental main.f90"
        library.write_text(module + "end module elemental_math\n", encoding="utf-8")
        program.write_text(main, encoding="utf-8")
        self.check_build([library, program], "2 4 6 7\n 12\n 4 12 24\n 3 6 9\n 24\n 0\n 8 8 8 8\n 24 24\n 1.0 0.648054 0.648054 1.0")

    def test_section_assignment(self):
        source = (ROOT / "tests" / "71_section_assignment.f90").read_text(encoding="utf-8")
        program = self.directory / "section assignment.f90"
        program.write_text(source, encoding="utf-8")
        self.check_build([program], "11 12 13 11 12 13 21 22 23\n 13 12 11 13 12 11 23 22 21\n -1 12 -1 13 12 11 -1 22 -1\n -2 34 -2\n 1 1 2 3 4\n 4 3 2 1 1\n 4 3 2 1 1\n 0 0 7 7 0 0 7 7\n 4.0 4.0 4.0 2.0 2.0 2.0\n 777\n 42 42 42 42 42")

    def test_forall_module(self):
        source = (ROOT / "tests" / "72_forall.f90").read_text(encoding="utf-8")
        main, procedures = source.split("\ncontains\n", 1)
        procedures = procedures.rsplit("end program forall_test", 1)[0]
        library = self.directory / "forall library.f90"
        program = self.directory / "forall main.f90"
        library.write_text("module forall_helpers\nimplicit none\ncontains\n" + procedures + "end module forall_helpers\n", encoding="utf-8")
        main = main.replace("program forall_test\n", "program forall_test\nuse forall_helpers, only: twice, checksum, scale_columns\n", 1)
        program.write_text(main + "\nend program forall_test\n", encoding="utf-8")
        self.check_build([library, program], "5 4 3 2 1 99\n 10 4 30 2 50\n 10 4 30 2 50\n 1 1 2 2 3 3\n 3 3 2 2 1 1\n 6.0 6.0 4.0 4.0 3.0 3.0\n 7\n 20 8 60 4 100\n 6")

    def test_output_implied_do(self):
        source = (ROOT / "tests/75_output_implied_do.f90").read_text(encoding="utf-8")
        program = self.directory / "output implied do.f90"
        program.write_text(source, encoding="utf-8")
        self.check_build([program], "values: 10 20 30 40 end 5\n 4 41 2 21 0\n nested: 11 12 21 22 3 3\n empty:done 3\n reverse empty:done 1\n chars:hi hi hi done\n blank:..\n\n array: 10 20 30 40 1 2 40 20\n 2 3 4 5")

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

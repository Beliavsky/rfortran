"""Formatted (A) IOSTAT, EOF/error recovery, and initialization at O0/O2."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from test_file_io import literal
from run_ofort_compat import equivalent


class RecordReadIOStat(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="record iostat (A&B) ")
        self.addCleanup(self.tmp.cleanup)
        self.directory = Path(self.tmp.name)

    def commands(self, program, reference=False):
        source = self.directory / "main.f90"
        source.write_text(program, encoding="utf-8")
        commands = build_commands(source, self.directory)
        if reference and shutil.which("gfortran"):
            exe = self.directory / ("reference.exe" if os.name == "nt" else "reference")
            result = subprocess.run(["gfortran", source, "-o", exe], cwd=self.directory,
                                    capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            commands.append([exe])
        return commands

    def execute(self, commands, data, expected=None, error=None):
        for command in commands:
            with self.subTest(command=command, data=data[:20]):
                result = subprocess.run(command, input=data, capture_output=True, timeout=15)
                if error:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(error.encode(), result.stderr)
                else:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stderr, b"")
                    self.assertTrue(equivalent(result.stdout.decode(), expected), result.stdout)

    def test_stdin_and_partial_final_record(self):
        program = (ROOT / "tests/122_record_read_iostat.f90").read_text(encoding="utf-8")
        commands = self.commands(program, reference=True)
        expected = "[abcde]\nT\n0 T\n[xy   ]\nT\nT\n"
        for data in (b"abcdef\nignored\nxy\n", b"abcdef\r\nignored\r\nxy\r\n", b"abcdef\nignored\nxy"):
            self.execute(commands, data, expected)

    def test_invalid_bytes_report_positive_status_and_consume_the_record(self):
        commands = self.commands("""implicit none
character(3) :: text
integer :: status
read(*,'(a)',iostat=status) text
print *, status > 0
read(*,'(a)',iostat=status) text
if (status == 0) print '(a)', '['//text//']'
print *, status == 0
end
""")
        for invalid in (b"\0", b"\xff", b"a"*12000+b"\xff"):
            self.execute(commands, invalid+b"\nok\n", "T\n[ok ]\nT\n")

    def test_file_units_aliasing_and_eof(self):
        data = self.directory / "input.txt"
        program = f"""implicit none
character(4) :: text
integer :: unit, status
unit = 20
read(unit,'(a)',iostat=status) text
print *, status > 0
open(unit,file={literal(data)},status='old',action='read')
read(unit,'(a)',iostat=unit) text
if (unit == 0) print '(a)', '['//text//']'
print *, unit == 0
read(20,'(a)',iostat=status) text
print *, status < 0
close(20)
unit = 1024
read(unit,'(a)',iostat=status) text
print *, status > 0
end
"""
        commands = self.commands(program)
        for contents in (b"hi\n", b"hi\r\n", b"hi"):
            data.write_bytes(contents)
            self.execute(commands, b"", "T\n[hi  ]\nT\nT\nT\n")

    def test_failed_read_leaves_destination_undefined(self):
        for initialized in (False, True):
            program = "implicit none\ncharacter(3)::text\ninteger::status\n"
            if initialized:
                program += "text='old'\n"
            program += "read(*,'(a)',iostat=status) text\nprint *,status\nprint *,text\nend\n"
            commands = self.commands(program)
            self.execute(commands, b"", error="uninitialized variable `text`")
            self.execute(commands, b"\xff\n", error="uninitialized variable `text`")
            self.execute(commands, b"ok\n", "0\nok\n")

    def test_procedure_read_with_output_arguments(self):
        commands = self.commands("""program main
implicit none
character(5) :: text
integer :: status
call get_text(5,text,status)
if (status == 0) print '(a)', '['//text//']'
print *, status == 0
contains
subroutine get_text(in_unit,text,status)
integer, intent(in) :: in_unit
character(*), intent(out) :: text
integer, intent(out) :: status
read(in_unit,'(a)',iostat=status) text
end subroutine get_text
end program main
""", reference=True)
        self.execute(commands, b"hello\n", "[hello]\nT\n")
        self.execute(commands, b"", "F\n")

    def test_diagnostics(self):
        for declarations, statement, message in [
            ("character::text\nreal::status", "read(*,'(a)',iostat=status) text", "IOSTAT must be a writable scalar INTEGER"),
            ("character::text\ninteger::status(2)", "read(*,'(a)',iostat=status) text", "IOSTAT must be a writable scalar INTEGER"),
            ("character::text", "read(*,'(a)',iostat=0) text", "IOSTAT must be a writable scalar INTEGER"),
            ("character::text\ninteger,parameter::status=0", "read(*,'(a)',iostat=status) text", "IOSTAT must be a writable scalar INTEGER"),
            ("integer::text,status", "read(*,'(a)',iostat=status) text", "CHARACTER variable"),
            ("character::text(2)\ninteger::status", "read(*,'(a)',iostat=status) text", "CHARACTER"),
            ("character::text\ninteger::status", "read(*,'(a4)',iostat=status) text", "only the (A) format"),
            ("character::text\ninteger::status", "read(*,'(a)',iomsg=status) text", "only the IOSTAT"),
            ("character::text\ninteger::status,unit", "read(unit,'(a)',iostat=status) text", "uninitialized variable `unit`"),
        ]:
            source = self.directory / "bad.f90"
            source.write_text("implicit none\n"+declarations+"\n"+statement+"\nend\n", encoding="utf-8")
            result = subprocess.run([COMPILER, "emit-obj", source, self.directory / "bad.obj"],
                                    capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(message, result.stderr)
        source.write_text("pure subroutine bad()\ncharacter::text\ninteger::status\nread(*,'(a)',iostat=status) text\nend subroutine bad\n", encoding="utf-8")
        result = subprocess.run([COMPILER, "emit-obj", source, self.directory / "bad.obj"],
                                capture_output=True, text=True, timeout=30)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("PURE procedure cannot perform external I/O", result.stderr)


if __name__ == "__main__":
    unittest.main()

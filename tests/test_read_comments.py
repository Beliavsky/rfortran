"""Comment-reader dependencies: scratch I/O and bounded CHARACTER arrays."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from test_file_io import literal
from run_ofort_compat import equivalent


class ReadComments(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="rfort-comments-")
        self.addCleanup(self.tmp.cleanup)
        self.directory = Path(self.tmp.name)

    def compare(self, program, expected=None, error=None, reference=True):
        source = self.directory / "main.f90"
        source.write_text(program, encoding="utf-8")
        commands = build_commands(source, self.directory)
        if reference and not error and shutil.which("gfortran"):
            exe = self.directory / ("reference.exe" if os.name == "nt" else "reference")
            result = subprocess.run(["gfortran", source, "-o", exe], cwd=self.directory,
                                    capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            commands.append([exe])
        for command in commands:
            with self.subTest(command=command):
                result = subprocess.run(command, cwd=self.directory, capture_output=True,
                                        text=True, timeout=30)
                if error:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(error, result.stderr)
                else:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stderr, "")
                    self.assertTrue(equivalent(result.stdout, expected), repr(result.stdout))

    def test_example(self):
        self.compare((ROOT / "tests/127_read_comments.f90").read_text(),
                     "comments read: 2\n! first comment\n!\nfirst non-comment: !not_a_comment\nread_comments tests passed\n")

    def test_newunit_positioning_and_reuse(self):
        self.compare("""implicit none
integer :: a, b, c, status
character(5) :: line
open(newunit=a,status='scratch')
open(newunit=b,status='scratch',action='readwrite')
print *, a<0, b<0, a/=b
write(a,'(a)') 'alpha','beta'
write(b,'(a)') 'other'
rewind(a)
read(a,'(a)') line
print '(a)',line
backspace(a)
read(a,'(a)') line
print '(a)',line
read(a,'(a)') line
print '(a)',line
read(a,'(a)',iostat=status) line
print *,status<0
rewind(b)
read(b,'(a)') line
print '(a)',line
close(a)
open(newunit=c,status='scratch')
print *,c<0,c/=b
close(b)
close(c)
open(unit=20,status='scratch')
write(20,'(a)') 'fixed'
rewind(20)
read(20,'(a)') line
print '(a)',line
close(20)
end
""", "T T T\nalpha\nalpha\nbeta\nT\nother\nT T\nfixed\n")

    def test_inquire_named_file_and_echo(self):
        data = self.directory / "input comments.txt"
        data.write_text("! hello\nvalue\n", encoding="ascii")
        module = (ROOT / "tests/127_read_comments.f90").read_text().split("program test_read_comments")[0]
        self.compare(module + f"""program main
use m
implicit none
character(300)::name
character(32),allocatable::comments(:)
character(5)::line
open(20,file={literal(data)},status='old',action='read')
inquire(unit=20,name=name)
print *,trim(name)=={literal(data)}
call read_comments(20,'!',echo=.true.,comments=comments)
print *,size(comments),trim(comments(1))=='! hello'
read(20,'(a)') line
print '(a)',line
close(20)
end
""", f"T\ncomments from file {str(data)[:32]}\n! hello\n\n1 T\nvalue\n")

    def test_output_replaces_remaining_records(self):
        self.compare("""implicit none
integer::unit,status
character(5)::line
open(newunit=unit,status='scratch')
write(unit,'(a)') 'first','second','third'
rewind(unit)
read(unit,'(a)') line
write(unit,'(a)') 'x'
rewind(unit)
read(unit,'(a)') line
print '(a)',line
read(unit,'(a)') line
print '(a)',line
read(unit,'(a)',iostat=status) line
print *,status<0
close(unit)
end
""", "first\nx    \nT\n")

    def test_eof_and_truncation(self):
        module = (ROOT / "tests/127_read_comments.f90").read_text().split("program test_read_comments")[0]
        self.compare(module + """program main
use m
implicit none
integer::iu
character(8),allocatable::comments(:)
open(newunit=iu,status='scratch')
write(iu,'(a)') '! abcdefghijkl','! end'
rewind(iu)
call read_comments(iu,'!',comments=comments)
print *,size(comments)
print '(a)',comments(1)
print '(a)',comments(2)
call read_comments(iu,'!',comments=comments)
print *,allocated(comments),size(comments)
close(iu)
end
""", "2\n! abcdef\n! end   \nT 0\n")

    def test_character_fields_nulls_quotes_and_status(self):
        self.compare("""implicit none
character(40)::source
character(5)::value
integer::status
source="'a b' rest"
read(source,*,iostat=status) value
print *,status
print '(a)',value
source='"a""b"'
read(source,*) value
print '(a)',value
source="3*'xy'"
read(source,*) value
print '(a)',value
source=','
read(source,*,iostat=status) value
print *,status
print '(a)',value
source='2*'
read(source,*) value
print '(a)',value
source='/'
read(source,*) value
print '(a)',value
source=' '
read(source,*,iostat=status) value
print *,status<0
source="0*'x'"
read(source,*,iostat=status) value
print *,status>0
end
""", '0\na b  \na"b  \nxy   \n0\nxy   \nxy   \nxy   \nT\nT\n')

    def test_dynamic_elements_and_loop_condition(self):
        self.compare("""implicit none
character(3)::a(-1:1)
integer::i
i=0
a(i)='yes'
print '(a)',a(i)
do while(a(i)=='yes')
  a(i)='end'
  cycle
end do
print '(a)',a(i)
a=a(i)
print *,all(a=='end')
end
""", "yes\nend\nT\n")

    def test_allocatable_character_array_input_and_reverse_section(self):
        program = """module copy_module
implicit none
contains
subroutine copy(a,b)
character(*),intent(in)::a(:)
character(*),allocatable,intent(out)::b(:)
allocate(b(size(a)))
b=a(size(a):1:-1)
end subroutine copy
end module copy_module
program main
use copy_module
implicit none
character(3)::a(3)
character(5),allocatable::b(:)
integer::i
a=['one','two','end']
call copy(a,b)
print *,size(b),allocated(b)
do i=1,size(b)
  print '(a)',b(i)
end do
end
"""
        expected = "3 T\nend  \ntwo  \none  \n"
        # A reverse copy needs explicit bounds: with omitted bounds, a(::-1)
        # runs from 1 down to 3 and is empty.
        self.compare(program.replace("b=a(size(a):1:-1)", "b=a(3:1:-1)"), expected)
        self.compare(program, expected)

    def test_dynamic_uninitialized_element_and_bounds(self):
        self.compare("""implicit none
character(3)::a(2)
integer::i
a(1)='yes'
i=2
print *,a(i)
end
""", error="element 2", reference=False)
        self.compare("""implicit none
character(3)::a(2)
integer::i
i=3
a(i)='bad'
end
""", error="index out of bounds", reference=False)

    def test_character_allocation_limits_and_empty_section(self):
        module = """module limits_module
implicit none
contains
subroutine make(n,b)
integer,intent(in)::n
character(*),allocatable,intent(out)::b(:)
character(3)::source(2)
source=['one','two']
allocate(b(n))
b=source(2:1:2)
end subroutine make
end module limits_module
"""
        main = """program main
use limits_module
implicit none
character(5),allocatable::b(:)
call make(0,b)
print *,allocated(b),size(b)
end
"""
        self.compare(module + main, "T 0\n")
        # The empty section reallocates B to size 0.
        self.compare(module + main.replace("make(0,b)", "make(3,b)"), "T 0\n")


if __name__ == "__main__":
    unittest.main(verbosity=2)

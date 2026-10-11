"""DEALLOCATE checks in the interpreter and native code at -O0 and -O2.

Run after cargo build --bin rfort: python tests/test_deallocate.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent


class Deallocate(unittest.TestCase):
    def test_fixture(self):
        fixture = ROOT / "tests/128_deallocate.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(
            encoding="utf-8"))["cases"][fixture.stem]["program_stdout"]
        self.check(fixture.read_text(encoding="utf-8"), expected)

    def check(self, program, expected=None, error=None, data=""):
        with tempfile.TemporaryDirectory(prefix="deallocate-") as tmp:
            directory = Path(tmp)
            source = directory / "example.f90"
            source.write_text(program, encoding="utf-8")
            commands = build_commands(source, directory)
            if expected is not None and shutil.which("gfortran"):
                exe = directory / "reference.exe"
                result = subprocess.run(["gfortran", "-std=f2018", "-fcheck=all",
                                         source, "-o", exe], cwd=directory,
                                        capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stderr)
                commands.append([exe])
            for command in commands:
                with self.subTest(command=command):
                    result = subprocess.run(command, input=data, capture_output=True,
                                            text=True, timeout=15)
                    if error is not None:
                        self.assertNotEqual(result.returncode, 0)
                        self.assertIn(error, result.stderr)
                    else:
                        self.assertEqual(result.returncode, 0, result.stderr)
                        self.assertEqual(result.stderr, "")
                        self.assertTrue(equivalent(result.stdout, expected), result.stdout)

    def reject(self, program, message):
        with tempfile.TemporaryDirectory(prefix="deallocate-invalid-") as tmp:
            source = Path(tmp) / "example.f90"
            source.write_text(program, encoding="utf-8")
            result = subprocess.run([COMPILER, "run", source], capture_output=True,
                                    text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(message, result.stderr)

    def test_local_array_reallocation(self):
        self.check("""program main
integer, allocatable :: a(:)
integer :: i
print *, allocated(a)
allocate(a(2))
a=[1,2]
print *, size(a),sum(a),allocated(a)
deallocate(a)
print *, allocated(a)
allocate(a(3))
a=4
print *,size(a),sum(a)
deallocate(a)
do i=1,3
  allocate(a(0))
  print *,allocated(a),size(a)
  if (allocated(a)) deallocate(a)
end do
print *,allocated(a)
end program
""", "F\n2 3 T\nF\n3 12\nT 0\nT 0\nT 0\nF\n")

    def test_specialized_procedure_local(self):
        self.check("""program main
integer :: a(3),b(2),total
a=[1,2,3]
b=[4,5]
call work(a,total)
print *,total
call work(b,total)
print *,total
contains
pure subroutine work(input,total)
integer,intent(in)::input(:)
integer,intent(out)::total
integer,allocatable::scratch(:)
integer::n
n=size(input)
allocate(scratch(n))
scratch=input
deallocate(scratch)
allocate(scratch(n))
scratch=input+1
total=sum(scratch)
deallocate(scratch)
if (allocated(scratch)) total=-999
end subroutine work
end program main
""", "9\n11\n")

    def test_mixed_character_and_numeric_objects(self):
        self.check("""program main
integer,allocatable::a(:),b(:)
character(len=:),allocatable::s,t
s='hello'
t='world'
allocate(a(2),b(0))
a=7
deallocate(s,a,t,b)
print *,allocated(a),allocated(b),allocated(s),allocated(t)
s='new'
allocate(a(1))
a=9
print *,s,len(s),a
deallocate(a,s)
end program
""", "F F F F\nnew 3 9\n")

    def test_bounded_output(self):
        module = (ROOT / "tests/123_recursive_allocatable.f90").read_text(
            encoding="utf-8").split("program test_pure_recursive")[0]
        self.check(module + """program main
use m
integer::a(4)
integer,allocatable::result(:)
a=[1,-2,3,4]
call set_alloc_int_vec(a,result,jmin=0)
print *,result
deallocate(result)
print *,allocated(result)
call set_alloc_int_vec(a,result,jmin=3)
print *,result
deallocate(result)
end program
""", "1 3 4\nF\n3 4\n")

    def test_unallocated_array(self):
        self.check("""integer,allocatable::a(:)
deallocate(a)
end
""", error="DEALLOCATE of unallocated array `a`")

    def test_double_deallocation(self):
        self.check("""integer,allocatable::a(:)
allocate(a(2))
deallocate(a)
deallocate(a)
end
""", error="DEALLOCATE of unallocated array `a`")

    def test_unallocated_character(self):
        self.check("""character(len=:),allocatable::s
s='hello'
deallocate(s)
deallocate(s)
end
""", error="DEALLOCATE of unallocated CHARACTER `s`")

    def test_invalid_targets_and_options(self):
        for target, message in (("", "at least one"), ("a(1)", "whole ALLOCATABLE"),
                                ("a,stat=i", "STAT/ERRMSG"),
                                ("a,errmsg=s", "STAT/ERRMSG")):
            with self.subTest(target=target):
                self.reject("integer,allocatable::a(:)\ninteger::i\n"
                            "character(80)::s\nallocate(a(2))\n"
                            f"deallocate({target})\nend\n", message)
        self.reject("integer::a(2)\ndeallocate(a)\nend\n", "ALLOCATABLE")

    def test_intent_in_rejected(self):
        self.reject("""program main
integer,allocatable::a(:)
allocate(a(2))
a=1
call work(a)
contains
subroutine work(a)
integer,allocatable,intent(in)::a(:)
deallocate(a)
end subroutine work
end program main
""", "INTENT(IN)")

    def test_contextual_identifier(self):
        self.check("""integer::deallocate(2)
deallocate=[3,4]
deallocate(1)=5
print *,deallocate
end
""", "5 4\n")

    def test_block_shadowing(self):
        self.check("""program main
integer,allocatable::a(:)
allocate(a(2))
a=3
block
integer,allocatable::a(:)
character(len=:),allocatable::s
allocate(a(1))
a=5
s='block'
print *,a,s,allocated(a),allocated(s)
deallocate(a,s)
print *,allocated(a)
end block
print *,a,allocated(a)
deallocate(a)
end program main
""", "5 block T T\nF\n3 3 T\n")

    def test_initialization_is_reset(self):
        self.check("""integer,allocatable::a(:)
integer::fill
allocate(a(2))
a=7
deallocate(a)
allocate(a(2))
read (*,*) fill
if (fill/=0) a=9
print *,a
end
""", error="uninitialized", data="0\n")

    def test_allocation_in_conditional(self):
        program = """integer,allocatable::a(:)
integer::small
read (*,*) small
if (small/=0) then
  allocate(a(2))
else
  allocate(a(3))
end if
a=4
print *,size(a),sum(a)
deallocate(a)
print *,allocated(a)
end
"""
        self.check(program, "2 8\nF\n", data="1\n")
        self.check(program, "3 12\nF\n", data="0\n")

    def test_pure_host_deallocation_rejected(self):
        self.reject("""program main
integer,allocatable::a(:)
allocate(a(2))
call work()
contains
pure subroutine work()
deallocate(a)
end subroutine work
end program main
""", "nonlocal")

    def test_nonallocatable_shadow_rejected(self):
        self.reject("""program main
integer,allocatable::a(:)
allocate(a(2))
block
integer::a(2)
deallocate(a)
end block
deallocate(a)
end program main
""", "ALLOCATABLE")

    def test_inout_deallocation_and_reallocation(self):
        self.check("""program main
integer,allocatable::a(:)
allocate(a(3))
a=7
call reset(a)
print *,a,allocated(a)
deallocate(a)
contains
pure subroutine reset(a)
integer,allocatable,intent(inout)::a(:)
deallocate(a)
allocate(a(2))
a=9
end subroutine reset
end program main
""", "9 9 T\n")

    def test_bounded_character_output(self):
        self.check("""program main
character(4)::input(2)
character(4),allocatable::output(:)
input=['ab  ','cd  ']
call copy(input,output)
print *,output(2)
deallocate(output)
print *,allocated(output)
call copy(input,output)
print *,output(1),allocated(output)
deallocate(output)
contains
subroutine copy(input,output)
character(4),intent(in)::input(:)
character(4),allocatable,intent(out)::output(:)
allocate(output(size(input)))
output=input
end subroutine copy
end program main
""", "cd\nF\nab T\n")

    def test_mixed_allocation_paths(self):
        self.check("""integer,allocatable::a(:),b(:,:)
allocate(a(2),b(2,2))
a=3
b=4
print *,sum(a),sum(b)
deallocate(a)
allocate(a(1))
a=5
print *,a,sum(b)
deallocate(a)
end
""", "6 16\n5 16\n")


if __name__ == "__main__":
    unittest.main(verbosity=2)

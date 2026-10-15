"""Recursive OPTIONAL calls, bounded allocatable outputs, and inlined RETURN.

Run after cargo build --bin rfort: python tests/test_recursive_allocatables.py
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

FIXTURE = ROOT / "tests/123_recursive_allocatable.f90"


class RecursiveAllocatables(unittest.TestCase):
    def compare(self, program, expected, data=""):
        with tempfile.TemporaryDirectory(prefix="recursive-allocatables-") as tmp:
            directory = Path(tmp)
            source = directory / "example.f90"
            source.write_text(program, encoding="utf-8")
            commands = build_commands(source, directory)
            if shutil.which("gfortran"):
                exe = directory / "reference.exe"
                result = subprocess.run(["gfortran", "-std=f2018", "-Wall", "-Wextra",
                                         "-fcheck=all", source, "-o", exe], cwd=directory,
                                        capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, "")
                commands.append([exe])
            for command in commands:
                with self.subTest(command=command):
                    result = subprocess.run(command, input=data, capture_output=True,
                                            text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stderr, "")
                    self.assertTrue(equivalent(result.stdout, expected), result.stdout)

    def module(self):
        return FIXTURE.read_text(encoding="utf-8").split("program test_pure_recursive")[0]

    def test_seven_original_checks(self):
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][FIXTURE.stem]["program_stdout"]
        self.compare(FIXTURE.read_text(encoding="utf-8"), expected)

    def test_runtime_bounds_and_nested_pack(self):
        program = self.module() + """program main
use m
integer :: a(6), lower, upper, i
integer, allocatable :: result(:)
a=[-3,0,2,2,5,9]
print *, allocated(result)
do i=1,3
  read (*,*) lower
  read (*,*) upper
  call set_alloc_int_vec(a,result,jmax=upper,jmin=lower)
  print *, size(result),sum(result),allocated(result)
end do
call set_alloc_int_vec(pack(a,a>=0),result,jmax=5)
print *, size(result),sum(result)
call set_alloc_int_vec(pack(pack(a,a>=0),pack(a,a>=0)<=5),result)
print *, result
end program main
"""
        self.compare(program, "F\n3 9 T\n2 -3 T\n0 0 T\n4 9\n0 2 2 5\n",
                     "2\n5\n-10\n0\n99\n100\n")

    def test_zero_capacity(self):
        self.compare(self.module() + """program main
use m
integer :: a(0)
integer,allocatable :: result(:)
call set_alloc_int_vec(a,result)
print *,allocated(result),size(result),sum(result),lbound(result,1),ubound(result,1)
call set_alloc_int_vec(a,result,jmin=3,jmax=9)
print *,allocated(result),size(result),sum(result),lbound(result),ubound(result)
end program main
""", "T 0 0 1 0\nT 0 0 1 0\n")

    def test_chained_outputs_capture_old_extent_and_scalar_assignment(self):
        self.compare(self.module() + """program main
use m
integer::a(6)
integer,allocatable::result(:),next(:)
a=[-3,0,2,2,5,9]
call set_alloc_int_vec(a,result)
call set_alloc_int_vec(result,next,jmin=2)
print *,size(next),sum(next)
call set_alloc_int_vec(a,result,jmin=size(result))
print *,size(result),sum(result)
result=7
result=result+2
print *,sum(result)
call set_alloc_int_vec(pack(a,a>0,vector=[99,98,97,96,95,94,93]),result)
print *,size(result),sum(result)
end program main
""", "4 18\n1 9\n9\n7 300\n")

    def test_return_from_nested_loops(self):
        self.compare("""program main
integer :: a(4),total
a=[1,2,-3,9]
call accumulate(a,total)
print *,total
call accumulate([1,2],total)
print *,total,77
contains
subroutine accumulate(a,total)
integer,intent(in)::a(:)
integer,intent(out)::total
integer::i,j
total=0
do i=1,size(a)
  do j=1,2
    if (a(i)<0) return
    total=total+a(i)
  end do
end do
total=total+10
end subroutine accumulate
end program main
""", "6\n16 77\n")

    def test_recursive_scalar_optional_function(self):
        self.compare("""program main
print *,accumulate(4),accumulate(4,step=2)
contains
recursive pure integer function accumulate(n,step) result(value)
integer,intent(in)::n
integer,intent(in),optional::step
if (n<=0) then
  value=0
else if (present(step)) then
  value=n*step+accumulate(n-1,step)
else
  value=n+accumulate(n-1)
end if
end function accumulate
end program main
""", "10 20\n")

    def test_invalid_allocation_and_unallocated_reads(self):
        for statements, module, error in [
            ("print *,size(result)\ncall set_alloc_int_vec(a,result)", self.module(), "unallocated array"),
            ("call set_alloc_int_vec(a,result)", self.module().replace("ni = size(ii)", "ni = size(jj)"), "unallocated array"),
            ("call set_alloc_int_vec(a,result)", self.module().replace("allocate (jj(ni))", "allocate (jj(ni))\n   allocate (jj(ni))"), "already allocated"),
            ("call set_alloc_int_vec(a,result)", self.module().replace("allocate (jj(ni))", "allocate (jj(ni+1))"), "shapes do not conform"),
            ("call set_alloc_int_vec(a,result)\nprint *,sum(result)", self.module().replace("jj = ii", "jj(1) = ii(1)"), "uninitialized variable"),
        ]:
            with self.subTest(error=error), tempfile.TemporaryDirectory(prefix="recursive-invalid-") as tmp:
                directory = Path(tmp)
                source = directory / "invalid.f90"
                source.write_text(module + "program main\nuse m\ninteger::a(2)\ninteger,allocatable::result(:)\na=[1,2]\n" + statements + "\nend program main\n", encoding="utf-8")
                if statements.startswith("print *,size(result)"):
                    result = subprocess.run([COMPILER, "emit-obj", source, directory / "invalid.obj"],
                                            capture_output=True, text=True, timeout=30)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("before ALLOCATE", result.stderr)
                    continue
                for command in build_commands(source, directory):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                    self.assertNotEqual(result.returncode, 0, result.stdout)
                    self.assertIn(error, result.stderr)

    def test_unsupported_array_recursion_and_sections_are_diagnosed(self):
        for module, statements, error in [
            (self.module(), "call set_alloc_int_vec(a,result)\nprint *,result(:)", "sections of bounded allocatable"),
            (self.module().replace("jj = ii", "jj = ii(:)"), "call set_alloc_int_vec(a,result,jmin=2)", "sections of bounded allocatable"),
            (self.module().replace("ni = size(ii)", "call set_alloc_int_vec(ii,jj)"), "call set_alloc_int_vec(a,result)", "recursive ALLOCATABLE/POINTER dummy"),
        ]:
            with self.subTest(error=error), tempfile.TemporaryDirectory(prefix="recursive-unsupported-") as tmp:
                directory = Path(tmp)
                source = directory / "unsupported.f90"
                source.write_text(module + "program main\nuse m\ninteger::a(2)\ninteger,allocatable::result(:)\na=[1,2]\n" + statements + "\nend program main\n", encoding="utf-8")
                result = subprocess.run([COMPILER, "emit-obj", source, directory / "unsupported.obj"],
                                        capture_output=True, text=True, timeout=30)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(error, result.stderr)


if __name__ == "__main__":
    unittest.main()

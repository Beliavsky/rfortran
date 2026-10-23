"""Runtime matrix allocation, logical extents and column-major indexing.

Run after cargo build --bin rfort: python tests/test_runtime_allocatables.py
Set RFORT_TEST_COMPILER to check the release build.
"""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

import test_deallocate
from test_record_read import ROOT, COMPILER, build_commands


class RuntimeAllocatables(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_fixture(self):
        fixture = ROOT / "tests/129_runtime_allocatables.f90"
        expected = json.loads((ROOT / "tests/expectations.json").read_text(
            encoding="utf-8"))["cases"][fixture.stem]["program_stdout"]
        self.check(fixture.read_text(encoding="utf-8"), expected)

    def test_original_xalloc(self):
        source = ROOT / "tests/io/runtime_allocatable_random.f90"
        with tempfile.TemporaryDirectory(prefix="xalloc-") as tmp:
            directory = Path(tmp)
            copy = directory / "xalloc.f90"
            copy.write_text(source.read_text(encoding="utf-8"), encoding="utf-8")
            for command in build_commands(copy, directory):
                with self.subTest(command=command):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=20)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stderr, "")
                    rows = result.stdout.splitlines()
                    self.assertEqual(len(rows), 3)
                    for row in rows:
                        values = row.split()
                        x, count = float(values[0]), int(values[1])
                        self.assertGreaterEqual(x, 0)
                        self.assertLess(x, 1)
                        self.assertEqual(count, 12 if x > 0.5 else 200)
                        self.assertEqual(len(values), count + 2)
                        self.assertTrue(all(float(v) >= 1 for v in values[2:]))

    def test_column_major_indices_and_changing_stride(self):
        self.check("""program main
integer,allocatable::a(:,:)
integer::i,j,iter,n,m
do iter=1,2
  if (iter==1) then
    n=2
    m=3
  else
    n=3
    m=2
  end if
  if (allocated(a)) deallocate(a)
  allocate(a(n,m))
  do j=1,m
    do i=1,n
      a(i,j)=10*j+i
    end do
  end do
  print *,a,a(n,m),sum(a)
end do
deallocate(a)
end program main
""", "11 12 21 22 31 32 32 129\n11 12 13 21 22 23 23 102\n")

    def test_random_number_only_fills_active_elements(self):
        self.check("""integer::n,m,iter
real,allocatable::a(:,:)
do iter=1,3
  if (iter==1) then
    n=3
    m=4
  else
    n=2
    m=1
  end if
  if (allocated(a)) deallocate(a)
  allocate(a(n,m))
  call random_number(a)
  print *,size(a),all(a>=0.0.and.a<1.0)
end do
deallocate(a)
end
""", "12 T\n2 T\n2 T\n")

    def test_random_number_element_destination(self):
        self.check("""real,allocatable::a(:,:)
integer::i,j
allocate(a(2,3))
i=2
j=3
call random_number(a(i,j))
print *,a(i,j)>=0.0.and.a(i,j)<1.0
deallocate(a)
end
""", "T\n")

    def test_zero_negative_extents_and_reuse(self):
        self.check("""integer,allocatable::a(:,:)
integer::n
n=-2
allocate(a(n,3))
print *,allocated(a),size(a),shape(a),lbound(a),ubound(a),sum(a)
deallocate(a)
allocate(a(2,0))
print *,allocated(a),size(a),shape(a),ubound(a)
deallocate(a)
allocate(a(2,2))
a=7
print *,sum(a)
deallocate(a)
end
""", "T 0 0 3 1 1 0 3 0\nT 0 2 0 2 0\n28\n")

    def test_rank_three(self):
        self.check("""integer,allocatable::a(:,:,:)
integer::i,j,k
allocate(a(2,3,2))
a=4
i=2
j=3
k=2
a(i,j,k)=9
print *,shape(a),size(a,3),a(i,j,k),sum(a)
deallocate(a)
allocate(a(1,2,3))
a=5
print *,shape(a),size(a),sum(a)
deallocate(a)
end
""", "2 3 2 2 9 53\n1 2 3 6 30\n")

    def test_conformable_runtime_array_arithmetic(self):
        self.check("""integer,allocatable::a(:,:),b(:,:)
allocate(a(2,3),b(2,3))
a=2
b=a+3
print *,sum(b),sum(a+b)
deallocate(a,b)
end
""", "30 42\n")

    def test_same_flat_size_different_shapes(self):
        for operation in ("print *,a+b",):
            with self.subTest(operation=operation):
                self.check("integer,allocatable::a(:,:),b(:,:)\n"
                           "allocate(a(2,3),b(3,2))\na=1\nb=2\n" + operation +
                           "\ndeallocate(a,b)\nend\n", error="shapes do not conform")
        # A whole ALLOCATABLE assignment reallocates to the RHS shape.
        self.check("integer,allocatable::a(:,:),b(:,:)\n"
                   "allocate(a(2,3),b(3,2))\na=1\nb=2\na=b\n"
                   "print *,shape(a),sum(a)\ndeallocate(a,b)\nend\n", "3 2 12\n")

    def test_unallocated_and_repeated_allocation(self):
        for operation, message in (("allocate(a(2,3))", "already allocated"),
                                   ("deallocate(a)\ndeallocate(a)", "unallocated array"),
                                   ("deallocate(a)\nprint *,size(a,2)", "unallocated array")):
            with self.subTest(operation=operation):
                source="integer,allocatable::a(:,:)\nallocate(a(2,3))\n" + operation + "\ndeallocate(a)\nend\n"
                if operation=="deallocate(a)\nprint *,size(a,2)":
                    self.reject(source,"used before ALLOCATE")
                else:
                    self.check(source,error=message)

    def test_axis_bounds_not_just_flat_offset(self):
        self.check("""integer,allocatable::a(:,:)
integer::i,j
allocate(a(2,3))
a=7
i=3
j=1
print *,a(i,j)
deallocate(a)
end
""", error="dimension 1: subscript outside allocated bounds")

    def test_reallocation_clears_element_initialization(self):
        self.check("""integer,allocatable::a(:,:)
integer::i
allocate(a(2,3))
a=7
deallocate(a)
allocate(a(1,2))
i=1
a(i,1)=9
print *,a(i,2)
deallocate(a)
end
""", error="uninitialized")

    def test_input_and_call_dependent_sizes(self):
        for setup,data,expected in (("read (*,*) n","7\n","7 2 14\n"),
                                     ("n=2\ncall change(n)","","10 2 20\n")):
            self.check("program main\ninteger::n\ninteger,allocatable::a(:,:)\n" +
                       setup + "\nallocate(a(n,2))\na=1\nprint *,shape(a),sum(a)\n"
                       "deallocate(a)\ncontains\nsubroutine change(n)\n"
                       "integer,intent(inout)::n\nn=10\nend subroutine change\n"
                       "end program main\n",expected,data=data)
        self.check("""integer::n,i
integer,allocatable::a(:,:)
n=2
do i=1,3
  n=n+1
  allocate(a(n,2))
  a=1
  print *,size(a),sum(a)
  deallocate(a)
end do
end
""","6 6\n8 8\n10 10\n")

    def test_unsupported_consumers_rejected(self):
        self.check("integer,allocatable::a(:,:)\nallocate(a(2,3))\na=2\n"
                   "print *,a(:,1)\ndeallocate(a)\nend\n", "2 2\n")
        for operation in ("print *,transpose(a)", "print *,sum(a,dim=1)",
                          "print *,sum(a,mask=a>0)",
                          "print *,a(1)"):
            with self.subTest(operation=operation):
                self.reject("integer,allocatable::a(:,:)\nallocate(a(2,3))\na=2\n" +
                            operation + "\ndeallocate(a)\nend\n",
                            "subscripts" if operation.endswith("a(1)") else "supported")

    def test_local_matrix_shadows_caller_storage(self):
        self.check("""program main
integer,allocatable::a(:,:)
integer::total
allocate(a(2,3))
a=7
call work(total)
print *,total,sum(a)
deallocate(a)
contains
pure subroutine work(total)
integer,intent(out)::total
integer,allocatable::a(:,:)
allocate(a(2,2))
a=3
total=sum(a)
deallocate(a)
end subroutine work
end program main
""", "12 42\n")

    def test_conditional_and_while_scope(self):
        self.check("""integer,allocatable::a(:,:)
integer::iter
allocate(a(2,3))
iter=0
do while(iter<2)
  iter=iter+1
  if (iter==1) then
    a=3
    print *,shape(a),sum(a)
  else
    a=5
    print *,shape(a),sum(a)
  end if
end do
deallocate(a)
end
""", "2 3 18\n2 3 30\n")

    def test_integer_bound_arithmetic(self):
        program="""integer,allocatable::a(:,:)
integer::selector,n,m
read (*,*) selector
if (selector==0) then
  n=-3
else
  n=4
end if
m=n+4
allocate(a(max(0,n),m))
a=1
print *,shape(a),size(a),sum(a)
deallocate(a)
end
"""
        self.check(program,"0 1 0 0\n",data="0\n")
        self.check(program,"4 8 32 32\n",data="1\n")

    def test_procedure_arguments_preserve_rank(self):
        for actual in ("a","a+1"):
            self.reject("program main\ninteger,allocatable::a(:,:)\n"
                        "allocate(a(2,3))\na=2\ncall show("+actual+")\n"
                        "deallocate(a)\ncontains\nsubroutine show(b)\n"
                        "integer,intent(in)::b(:)\nprint *,sum(b)\n"
                        "end subroutine show\nend program main\n","type or rank mismatch")
        self.check("""program main
integer,allocatable::a(:,:)
allocate(a(2,3))
a=2
call show(a)
deallocate(a)
contains
subroutine show(b)
integer,intent(in)::b(:,:)
print *,size(b,1),size(b,2),sum(b)
end subroutine show
end program main
""","2 3 12\n")

    def test_assignment_of_expressions_allocates(self):
        self.check("""program ra
implicit none
real, allocatable :: c(:,:), t(:,:), v(:), w(:)
integer, allocatable :: k(:)
allocate(c(2,3), v(4))
c = 1.0
c(2,3) = 5.0
v = [1.0, 2.0, 3.0, 4.0]
t = c + 1
print *, shape(t), t
t = 2*c - t
print *, shape(t), t
t = sqrt(c)
print *, t
w = v*2 + 1
print *, size(w), w
w = abs(v - 10)
print *, w
k = int(v) * 3
print *, k
w = v(2:3) + 1
print *, w
end program
""", "2 3 2.0 2.0 2.0 2.0 2.0 6.0\n2 3 0.0 0.0 0.0 0.0 0.0 4.0\n"
            "1.0 1.0 1.0 1.0 1.0 2.23606801\n4 3.0 5.0 7.0 9.0\n9.0 8.0 7.0 6.0\n3 6 9 12\n3.0 4.0\n")
        self.check("""program rc
implicit none
real, allocatable :: c(:,:), t(:,:)
integer :: i
allocate(c(0:1,3), t(5,5))
c = reshape([(real(i), i = 1, 6)], [2,3])
t = c * 2
print *, shape(t), lbound(t), t
t = t + 10
print *, t
t = -t / 2 + c
print *, t
deallocate(t)
t = sqrt(c) + 1
print *, lbound(t), t
end program
""", "2 3 1 1 2.0 4.0 6.0 8.0 10.0 12.0\n12.0 14.0 16.0 18.0 20.0 22.0\n"
            "-5.0 -5.0 -5.0 -5.0 -5.0 -5.0\n1 1 2.0 2.41421366 2.73205090 3.0 3.23606801 3.44948983\n")


if __name__ == "__main__":
    unittest.main(verbosity=2)

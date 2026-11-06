"""MOVE_ALLOC and the SOURCE=, MOLD=, STAT= and ERRMSG= allocation options.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_allocation_options.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class AllocationOptions(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_source_preserves_explicit_bounds(self):
        self.check('''implicit none
integer,allocatable::a(:)
allocate(a(-1:1),source=[2,3,4])
print *,lbound(a),a
end
''','-1 2 3 4')

    def test_source_allocation_inquiry_is_evaluated_before_allocate(self):
        self.check('''implicit none
logical,allocatable::a(:)
allocate(a(2),source=allocated(a))
print *,a
end
''','F F')

    def test_invalid_controls(self):
        for statement,message in (
            ('allocate(a(2),stat=s,stat=s)','duplicate ALLOCATE option'),
            ('allocate(a,source=[1,2],mold=[1,2])','mutually exclusive'),
            ('deallocate(a,stat=s,stat=s)','duplicate DEALLOCATE option')):
            with self.subTest(statement=statement):
                self.reject('implicit none\ninteger,allocatable::a(:)\ninteger::s\n'+statement+'\nend\n',message)

    def test_move_preserves_numeric_initialization_flags(self):
        for kind,value in [('integer','7'),('real','1.0'),('logical','.true.')]:
            with self.subTest(kind=kind):
                self.check(f'''implicit none
{kind},allocatable::a(:),b(:)
integer::n
read *,n
allocate(a(n))
a(1)={value}
call move_alloc(from=a,to=b)
print *,b
end
''',error='uninitialized variable `b`: element 2',data='3\n')

    def test_options_and_move_alloc(self):
        self.check("""program al2
implicit none
type :: t
  integer, allocatable :: a(:)
end type t
type(t) :: x, y
integer, allocatable :: a(:), b(:), c(:,:), d(:,:)
integer :: s
character(len=40) :: m
allocate(a(-1:1), stat=s, errmsg=m)
print *, s == 0, lbound(a), size(a)
a = [4, 5, 6]
allocate(a(2), stat=s, errmsg=m)
print *, s /= 0, len_trim(m) > 0
allocate(b, source=a)
print *, lbound(b), b
deallocate(b)
allocate(b, mold=a)
print *, size(b), lbound(b)
call move_alloc(a, b)
print *, allocated(a), allocated(b), lbound(b), b
deallocate(a, stat=s)
print *, s /= 0
allocate(c(2,3))
c = 7
call move_alloc(c, d)
print *, allocated(c), shape(d), sum(d)
allocate(x%a(2))
x%a = [1, 2]
call move_alloc(x%a, y%a)
print *, allocated(x%a), y%a
end program al2
""", 'T -1 3\nT T\n-1 4 5 6\n3 -1\nF T -1 4 5 6\nT\nF 2 3 42\nF 1 2\n')


if __name__ == "__main__":
    unittest.main()

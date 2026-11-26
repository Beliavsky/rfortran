"""CHARACTER array constructors with variables, intrinsic results and arrays
as items, assigned to fixed-size and ALLOCATABLE arrays and passed as
actual arguments, in the interpreter, native O0/O2 and gfortran."""
import unittest
import test_deallocate


class CharacterConstructorItems(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_fixed_size_targets(self):
        self.check('''program p
implicit none
character(len=6) :: c(2), s, d(4)
s = 'ab'
c = [s, s]
print *, c
c = [repeat('x', 6), s]
print *, c
c = [adjustr(s), s(1:4)//'zz']
print *, c
d = [c, c(2), 'qq    ']
print *, d
call show([s, adjustr(s)])
contains
subroutine show(v)
character(len=*), intent(in) :: v(:)
print *, size(v), v
end subroutine
end program
''', ' ab    ab    \n xxxxxxab    \n     abab  zz\n     abab  zzab  zzqq    \n 2 ab        ab\n')

    def test_appending_to_allocatable_arrays(self):
        # An item that is the target itself, with intrinsic results.
        self.check('''program p
implicit none
integer, parameter :: nc = 6
character(len=nc), allocatable :: tags(:)
character(len=nc) :: s
s = 'ab'
allocate (tags(0))
tags = [tags, repeat('x', nc)]
tags = [tags, adjustl(s)]
tags = [tags, adjustr(s)]
print *, size(tags)
print '(3("[",a,"]"))', tags
end program
''', '3\n[xxxxxx][ab    ][    ab]\n')

    def test_substring_and_runtime_length_items(self):
        # Items of the same length as substrings and concatenations of them,
        # with constant bounds, and with lengths known only at run time.
        self.check('''program p
implicit none
character(len=6) :: s, c(2)
character(len=8) :: u
integer :: k
s = 'abcdef'
u = 'xyzxyz'
k = 2
call show([s, s(1:2)//'cdef'])
print *, [s, s(3:4)//s(1:4)]
c = ['cd'//s(1:4), s(:2)//'cd'//s(5:)]
print *, c
call show([s, s(1:k)//'cdef'])
print *, [s, s(1:k)//'cdef']
c = [s, s(1:k)//'wxyz']
print *, c
call show([s, trim(u)])
c = [trim(u), s]
print *, c
contains
subroutine show(v)
character(len=*), intent(in) :: v(:)
print *, size(v), v
end subroutine
end program
''', '2 abcdefabcdef\nabcdefcdabcd\ncdabcdabcdef\n2 abcdefabcdef\nabcdefabcdef\nabcdefabwxyz\n2 abcdefxyzxyz\nxyzxyzabcdef\n')

    def test_typed_constructors_with_allocatable_items(self):
        # ALLOCATABLE CHARACTER array items, local and of a module, take the
        # constructor's length.
        self.check('''module m
character(len=4), allocatable :: g(:)
end module
program p
use m
implicit none
allocate (g(1))
g(1) = 'zz'
g = [character(len=4) :: g, 'ab']
print *, size(g), len(g), '[', g, ']'
call add('cdefgh')
contains
subroutine add(s)
character(len=*), intent(in) :: s
character(len=4), allocatable :: names(:)
allocate (names(1))
names(1) = 'yy'
names = [character(len=4) :: names, s, g]
print *, size(names), '[', names, ']'
names = [character(len=2) :: names]
print *, size(names), '[', names, ']'
end subroutine
end program
''', '2 4 [zz ab ]\n4 [yy cdefzz ab ]\n4 [yy cd zz ab ]\n')

    def test_typed_constructors_allocate_the_target(self):
        self.check('''program p
implicit none
character(len=6), allocatable :: tags(:)
character(len=6) :: s
s = 'ab'
tags = [character(len=6) :: s, 'cd']
print *, size(tags), tags
tags = [character(len=6) :: repeat('x', 3)]
print *, size(tags), tags
tags = [character(len=4) :: s, 'abcdefgh']
print *, size(tags), tags
end program
''', '2 ab    cd\n1 xxx\n2 ab    abcd\n')


    def test_section_items_in_comparisons(self):
        self.check('''program p
implicit none
character(len=6), parameter :: names(4) = ['value ','delta ','vega  ','parity']
character(len=6) :: g
character(len=20) :: buf
integer :: n
g = 'delta'
buf = 'parity'
n = 6
print *, any([names(:2), names(4)] == g), count([names(3:), names(1)] == g)
print *, any([names(:2), names(4)] == buf(1:n)), all([names(4:4), names(4)] == buf(1:n))
end program p
''', "T 0\nT T\n")

    def test_section_and_array_items_in_subroutine_actuals(self):
        # Sections and whole arrays among the items of a constructor passed to
        # an assumed-shape CHARACTER dummy, here an OPTIONAL keyword actual.
        self.check('''module m
implicit none
character(len=11), parameter :: labels(3) = ["strike     ", "#days      ", "first_stock"]
contains
subroutine show(outu, extra)
integer, intent(in) :: outu
character(len=*), intent(in), optional :: extra(:)
integer :: i
if (present(extra)) then
   write (outu, "(*(a,'|'))") (trim(extra(i)), i = 1, size(extra))
else
   write (outu, "(a)") "none"
end if
end subroutine
end module
program p
use m
implicit none
character(len=11) :: v(2)
v = ["v1         ", "v2         "]
call show(6, extra=["first_date ", "last_date  ", labels(2:)])
call show(6, extra=[v, labels(3:1:-2), v(2:2)])
call show(6, [labels, labels(:1)])
call show(6)
end program p
''', "first_date|last_date|#days|first_stock|\nv1|v2|first_stock|strike|v2|\nstrike|#days|first_stock|strike|\nnone\n")

if __name__ == '__main__':
    unittest.main(verbosity=2)

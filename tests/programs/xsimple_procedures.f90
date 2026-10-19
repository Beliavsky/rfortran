module simple_examples
implicit none
integer, parameter :: factor = 2
contains
real simple elemental function convert(a) result(b)
integer, intent(in) :: a
b = sqrt(real(factor*a))
end function
simple subroutine delimiter(text, pos)
character(*), intent(in) :: text
integer, intent(out) :: pos
pos = 0
call split(text, ",", pos)
end subroutine
end module

program test_simple
use simple_examples
implicit none
integer :: pos
print "(3f7.3)", convert(2), convert(8), convert(18)
call delimiter("abc,def", pos)
print *, pos
end program

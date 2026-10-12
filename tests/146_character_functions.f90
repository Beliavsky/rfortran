module m
implicit none
contains
pure function f(a, b) result(ab)
character (len=*), intent(in) :: a, b
character (len=len_trim(a) + len_trim(b)) :: ab
ab = trim(a) // trim(b)
end function f

pure function g(a, b) result(ab)
character (len=*), intent(in) :: a, b
character (len=:), allocatable :: ab
ab = trim(a) // trim(b)
end function g
end module m

use m
print "(a)", "'" // f("abc  ", " def ") // "'"
print "(a)", "'" // g("abc  ", " def ") // "'"
end

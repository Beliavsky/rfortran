! A user generic named REPEAT must retain the intrinsic REPEAT overload.
! Expected output: [000].
module repeat_mod
implicit none
interface repeat
   module procedure repeat_char
end interface
contains
pure function repeat_char(n, xx) result(yy)
integer, intent(in) :: n
character(len=*), intent(in) :: xx
character(len=len(xx)) :: yy(n)
integer :: i
yy = [(xx, i=1,n)]
end function repeat_char
end module repeat_mod

program xrepeat_intrinsic
use repeat_mod, only: repeat
implicit none
! This is the intrinsic signature; the module overload takes (INTEGER, CHARACTER).
print *, '[' // repeat('0', 3) // ']'
end program xrepeat_intrinsic

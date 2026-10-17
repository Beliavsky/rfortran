! Reproduces an allocatable CHARACTER array result from a defined operator.
! Expected output: dog boy.
module character_exclude_mod
implicit none
interface operator(.exclude.)
   module procedure diff_vec_char
end interface
contains
function diff_vec_char(xx,yy) result(zz)
character(len=*), intent(in) :: xx(:),yy(:)
character(len=len(xx)), allocatable :: zz(:)
logical :: xuse(size(xx))
integer :: i
forall(i=1:size(xx)) xuse(i) = all(yy /= xx(i))
call set_alloc(pack(xx,xuse),zz)
end function diff_vec_char

pure subroutine set_alloc(xx,yy)
character(len=*), intent(in) :: xx(:)
character(len=*), allocatable, intent(out) :: yy(:)
allocate(yy(size(xx)))
yy = xx
end subroutine set_alloc
end module character_exclude_mod

program main
use character_exclude_mod, only: operator(.exclude.)
implicit none
write(*,"(100(a,1x))") ["dog","cat","boy"] .exclude. ["cat "]
end program main

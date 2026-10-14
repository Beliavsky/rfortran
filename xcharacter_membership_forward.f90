! Reproduces assumed-shape CHARACTER arguments forwarded to an array function.
! Expected output: F T F.
module membership_forward_mod
implicit none
interface operator(.in.)
   module procedure char_vec_in_vec
end interface
contains
pure function char_vec_in_vec(jvec,ivec) result(tf)
character(len=*), intent(in) :: jvec(:),ivec(:)
logical :: tf(size(jvec))
integer :: i
forall(i=1:size(jvec)) tf(i) = any(jvec(i) == ivec)
end function char_vec_in_vec

subroutine membership(xfind,xdata,mask)
character(len=*), intent(in) :: xfind(:),xdata(:)
logical, intent(out) :: mask(:)
mask = xfind .in. xdata
end subroutine membership
end module membership_forward_mod

program main
use membership_forward_mod, only: membership
implicit none
logical :: mask(3)
call membership(["dog","cat","ant"],["cat","fox"],mask)
print *,mask
end program main

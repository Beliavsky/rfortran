! Reproduces assumed-shape function arguments forwarded by a subroutine.
! Expected output: 2 (the only value absent from [1,3]).
module membership_forward_mod
implicit none
contains
pure function not_in_vector(i,values) result(tf)
integer,intent(in)::i,values(:)
logical::tf
tf=.not. any(i==values)
end function not_in_vector
subroutine check_values(ii,jj)
integer,intent(in)::ii(:),jj(:)
integer::i
do i=1,size(ii)
   if (not_in_vector(ii(i),jj)) print *,ii(i)
end do
end subroutine check_values
end module membership_forward_mod

program main
use membership_forward_mod,only:check_values
implicit none
call check_values([1,2,3],[1,3])
end program main

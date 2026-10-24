! Regression for the output implied-DO bound blocker in xacv.f90.
! Expected output: min max min max
module output_implied_do_bound_mod
implicit none
contains
subroutine show(words)
character(len=*),intent(in)::words(:)
character(len=3),allocatable::local(:)
integer::i,n
n=size(words)
allocate(local(n))
local=words
write(*,'(100(a,1x))') (trim(local(i)),i=1,size(local)), &
                     (trim(words(i)),i=1,size(words))
end subroutine
end module

program test_output_implied_do_bound
use output_implied_do_bound_mod
implicit none
call show(['min','max'])
end program

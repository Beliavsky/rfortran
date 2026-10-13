! Reproduce generic SET_ALLOC dispatch for an INTEGER implied-DO constructor.
! Expected output: 1 2 3.
module set_alloc_mod
implicit none
interface set_alloc
   module procedure set_alloc_int_vec
end interface
contains
pure subroutine set_alloc_int_vec(ii, jj)
integer, intent(in) :: ii(:)
integer, allocatable, intent(out) :: jj(:)
allocate(jj(size(ii)))
jj = ii
end subroutine set_alloc_int_vec
end module set_alloc_mod

program xset_alloc_implied_do
use set_alloc_mod, only: set_alloc
implicit none
integer :: i, n
integer, allocatable :: ivec(:)
n = 3
call set_alloc([(i, i=1,n)], ivec)
print *, ivec
end program xset_alloc_implied_do

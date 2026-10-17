! Reproducer for util.f90: gen_comb allocates an INTENT(OUT) matrix dummy.
! Expected output:
! T 0 3 0
! T 2 2 4
! T 3 2 6
module matrix_factory
implicit none
contains
subroutine make_matrix(n,k,imat)
integer, intent(in) :: n,k
integer, allocatable, intent(out) :: imat(:,:)
if (k > n) then
   if (allocated(imat)) deallocate(imat)
   allocate(imat(0,k))
   return
end if
allocate(imat(n,k))
imat=1
end subroutine make_matrix
end module matrix_factory

program test_allocatable_dummy_matrix
use matrix_factory, only: make_matrix
implicit none
integer, allocatable :: a(:,:)
call make_matrix(2,3,a)
print *,allocated(a),shape(a),sum(a)
! INTENT(OUT) deallocates an allocated actual on entry to the procedure.
call make_matrix(2,2,a)
print *,allocated(a),shape(a),sum(a)
call make_matrix(3,2,a)
print *,allocated(a),shape(a),sum(a)
deallocate(a)
end program test_allocatable_dummy_matrix

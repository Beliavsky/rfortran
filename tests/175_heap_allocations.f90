! Reproducer for util.f90: combos allocates a module array using choose().
! Expected output: 6 2 12
module combinations_mod
implicit none
integer, allocatable :: comb_mat(:,:)
contains
pure integer function choose(n,k) result(j)
integer, intent(in) :: n,k
integer :: i
j=1
do i=1,k
   j=j*(n-i+1)/i
end do
end function choose

subroutine combos(n_max,m_max)
integer, intent(in) :: n_max,m_max
integer :: nchoose
if (allocated(comb_mat)) deallocate(comb_mat)
nchoose=choose(n_max,m_max)
allocate(comb_mat(nchoose,m_max))
comb_mat=1
end subroutine combos
end module combinations_mod

program test_comb_mat_alloc
use combinations_mod, only: combos,comb_mat
implicit none
call combos(4,2)
print *,size(comb_mat,1),size(comb_mat,2),sum(comb_mat)
deallocate(comb_mat)
end program test_comb_mat_alloc

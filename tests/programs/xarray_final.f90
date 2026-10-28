! Whole-array FINAL dispatch, elemental fallback and assignment sequencing.
module array_final_demo
implicit none
type :: item
   integer :: value = 7
contains
   final :: finish_element, finish_vector
end type
contains
impure elemental subroutine finish_element(x)
   type(item), intent(inout) :: x
   print *, 'element', x%value
end subroutine
subroutine finish_vector(x)
   type(item), intent(inout) :: x(:)
   print *, 'vector', size(x), sum(x%value)
end subroutine
end module

program demo
use array_final_demo
implicit none
type(item), allocatable :: empty(:), matrix(:,:)
block
type(item) :: x(3)
x%value = [1,2,3]
! Preserve the right-hand side before finalizing the selected destination.
x(2:3) = x(1:2)
print *, 'values', x%value
end block
allocate(empty(0))
deallocate(empty) ! The rank-one finalizer is called even for an empty array.
allocate(matrix(2,1))
matrix%value = reshape([8,9],[2,1])
deallocate(matrix) ! No rank-two finalizer: use the elemental fallback.
end program

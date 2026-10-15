! Reproducer: a row section of a runtime-sized assumed-shape dummy.
program xruntime_matrix_section
implicit none
integer, allocatable :: matrix(:,:)
integer :: n

n = 3
allocate(matrix(2,n))
matrix(1,:) = [1,2,3]
matrix(2,:) = [4,5,6]
call print_first_row(matrix)

contains

subroutine print_first_row(a)
integer, intent(in) :: a(:,:)
print '(3(i0,1x))', a(1,:)
end subroutine print_first_row

end program xruntime_matrix_section

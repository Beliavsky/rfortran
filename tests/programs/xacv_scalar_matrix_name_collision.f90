! Repro for the generated @matrix_alloc_xx_0_length error in xacv.f90:14210.
! The scalar xx and the main program's matrix xx are separate entities.
! Expected output: 1.2500
module scalar_calculation_mod
implicit none
contains
elemental function scalar_value(x) result(answer)
double precision, intent(in) :: x
double precision :: answer
call evaluate(x, answer)
end function scalar_value

elemental subroutine evaluate(x, answer)
double precision, intent(in) :: x
double precision, intent(out) :: answer
double precision :: xx
xx = x*x
answer = xx + 1d0
end subroutine evaluate
end module scalar_calculation_mod

program test_scalar_matrix_name_collision
use scalar_calculation_mod, only: scalar_value
implicit none
double precision, allocatable :: xx(:,:)
allocate(xx(2,2))
xx = 1d0
print '(f8.4)', scalar_value(0.5d0)
deallocate(xx)
end program test_scalar_matrix_name_collision

! Warning:    could-be-parameter
! Enabled by: -Wstyle
! A local variable set once from a constant expression and never modified.
! As a named constant (PARAMETER) it cannot be changed by mistake and can
! be used in declarations.
program could_be_parameter
implicit none
integer :: n_max            ! expect: could-be-parameter
integer :: n
n_max = 10
n = 0
do while (n < n_max)
   n = n + 3
end do
print *, n
end program could_be_parameter

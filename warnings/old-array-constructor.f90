! Warning:    old-array-constructor
! Enabled by: -Wstyle
! An array constructor written (/ ... /), the Fortran 90 form. Since
! Fortran 2003, [ ... ] is shorter and clearer.
program old_array_constructor
implicit none
integer :: a(3), b(3)
a = (/ 1, 2, 3 /)          ! expect: old-array-constructor
b = [1, 2, 3]
print *, a + b
end program old_array_constructor

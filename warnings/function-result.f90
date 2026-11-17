! Warning:    function-result
! Enabled by: -Wstyle
! A FUNCTION statement with the result type in its prefix, as
! `real function f(x)`, or without a RESULT clause. The form
! `function f(x) result(y)` with y declared in the body keeps all the
! declarations together and the function name free for recursion.
program function_result
implicit none
print *, square(3.0), cube(2.0)
contains
real function square(x)              ! expect: function-result
real, intent(in) :: x
square = x*x
end function square
function cube(x) result(y)
real, intent(in) :: x
real :: y
y = x*x*x
end function cube
end program function_result

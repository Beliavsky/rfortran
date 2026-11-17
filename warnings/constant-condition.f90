! Warning:    constant-condition
! Enabled by: -Wextra
! A condition, or part of one, whose value is known when compiling, so that
! part of the test has no effect: a literal combined with another operand,
! a variable compared with itself, or comparisons that contradict each
! other. Named constants, which are usually settings, are not reported.
program constant_condition
implicit none
logical, parameter :: verbose = .false.
integer :: n
real :: x, y
n = 4
x = 1.0
y = 2.0
if (.true. .or. x > y) print *, "always"     ! expect: constant-condition
if (n == n) print *, "always"                ! expect: constant-condition
if (n > 5 .and. n < 3) print *, "never"      ! expect: constant-condition
if (verbose .or. x > y) print *, "x > y"
if (x /= x) print *, "x is a NaN"
end program constant_condition

! Warning:    constant-condition-all
! Enabled by: -Weverything
! A condition whose value is known from named constants, such as a debug
! setting. -Wconstant-condition, in -Wextra, does not report named
! constants, since they are usually settings; this reports them too.
program constant_condition_all
implicit none
logical, parameter :: debug = .false.
integer :: n
n = 3
if (debug .or. n > 2) print *, "n > 2"      ! expect: constant-condition-all
if (n > 2) print *, "n > 2"
end program constant_condition_all

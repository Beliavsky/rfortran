program fortran2023_basics
use, intrinsic :: iso_fortran_env, only: logical32, real16
implicit none
integer, parameter :: lk = selected_logical_kind(bits=1)
logical(kind=lk) :: available
character(len=20) :: message = "Hello from rfort"

available = lk > 0
print "(at)", message
print *, available
print *, lk, logical32, real16
end program fortran2023_basics

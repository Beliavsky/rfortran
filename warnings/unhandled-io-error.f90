! Warning:    unhandled-io-error
! Enabled by: -Weverything
! In a procedure, an OPEN without IOSTAT= or ERR=, or a READ without
! IOSTAT=, ERR= or END=. A missing file or invalid data then stops the
! program, taking the decision away from the caller. Converting text to a
! number with an internal READ is the common case.
program unhandled_io_error
implicit none
print *, to_integer("42"), to_integer_checked("4x2")
contains
function to_integer(text) result(n)
character(len=*), intent(in) :: text
integer :: n
read (text, *) n                    ! expect: unhandled-io-error
end function to_integer
function to_integer_checked(text) result(n)
character(len=*), intent(in) :: text
integer :: n, ios
read (text, *, iostat=ios) n
if (ios /= 0) n = -1
end function to_integer_checked
end program unhandled_io_error

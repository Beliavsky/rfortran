! Warning:    implicit-interface
! Enabled by: -Weverything
! A call of an external procedure without an explicit interface. The
! compiler cannot check the number, types or intents of the arguments. Put
! the procedure in a module, or give it an interface block.
module greetings
implicit none
contains
subroutine greet(name)
character(len=*), intent(in) :: name
print *, "hello, ", name
end subroutine greet
end module greetings

subroutine farewell(name)
implicit none
character(len=*), intent(in) :: name
print *, "goodbye, ", name
end subroutine farewell

program implicit_interface
use greetings, only: greet
implicit none
call greet("Ada")
call farewell("Ada")        ! expect: implicit-interface
end program implicit_interface

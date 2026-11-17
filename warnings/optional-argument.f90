! Warning:    optional-argument
! Enabled by: -Wall
! An OPTIONAL dummy argument used where nothing proves it is PRESENT. A
! reference to an absent argument is an error that compilers need not
! detect.
program optional_argument
implicit none
call greet("Ada")
call greet("Grace", "Dr.")
contains
subroutine greet(name, title)
character(len=*), intent(in) :: name
character(len=*), intent(in), optional :: title
if (present(title)) then
   print *, title, " ", name
else
   print *, name
end if
print *, len(title)    ! expect: optional-argument
end subroutine greet
end program optional_argument

program main
implicit none
enumeration type :: colour
   enumerator :: red, orange, green
end enumeration type
integer :: n
read *, n
call exercise(n)
contains
subroutine exercise(n)
integer, intent(in) :: n
type(colour) :: lights(n), advanced(n)
integer :: statuses(n)

lights = orange
if (n > 0) lights(n) = green
advanced = next(lights, stat=statuses)
print *, int(advanced)
print *, statuses

advanced = previous(lights, stat=statuses)
print *, int(advanced)
print *, statuses
end subroutine
end program

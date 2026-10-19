program main
implicit none
enumeration type :: colour
   enumerator :: red, orange, green
end enumeration type
type(colour) :: light, lights(3), advanced(3)
integer :: status, statuses(3)

light = next(red)
print *, int(light)
light = previous(green)
print *, int(light)

! A boundary leaves the value unchanged and reports a positive status.
light = next(a=green, stat=status)
print *, int(light), status > 0
light = previous(a=red, stat=status)
print *, int(light), status > 0

! Elemental calls return a status for each array element.
lights = [colour :: red, orange, green]
advanced = next(lights, statuses)
print *, int(advanced)
print *, statuses
end program

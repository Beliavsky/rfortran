program example
implicit none
enumeration type :: colour
   enumerator :: red, orange, green
end enumeration type
type(colour), allocatable :: lights(:), moved(:)
integer, allocatable :: statuses(:)
integer :: n

read *, n
print *, allocated(lights)
allocate(lights(-1:n-2), statuses(n))
lights = orange
lights = next(lights, stat=statuses)
print *, int(lights), statuses
call move_alloc(lights, moved)
print *, allocated(lights), allocated(moved), size(moved)
deallocate(moved, statuses)
end program example

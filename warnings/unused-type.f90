! Warning:    unused-type
! Enabled by: -Wall
! A derived type that nothing uses: no variable, component, constructor or
! other type refers to it.
program unused_type
implicit none
type :: point
   real :: x = 0, y = 0
end type point
type :: segment    ! expect: unused-type
   type(point) :: a, b
end type segment
type(point) :: p
p = point(1.0, 2.0)
print *, p%x + p%y
end program unused_type

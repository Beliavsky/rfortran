! Warning:    format
! Enabled by: -Wall
! A format whose edit descriptors do not match the types of the items it
! formats, which stops the program with an error at run time. Literal
! formats, CHARACTER constants and FORMAT statements are checked.
program format
implicit none
integer :: n
real :: x
character(len=*), parameter :: fmt = "(a, i0)"
n = 3
x = 2.5
print "(i4, f8.3)", n, x
print "(i4, f8.3)", x, n       ! expect: format
print fmt, "n =", n
print fmt, "x =", x            ! expect: format
write (*, 10) n, n             ! expect: format
10 format (i4, f8.3)
end program format

! Warning:    format-unused
! Enabled by: -Wextra
! Data edit descriptors that no item of the list reaches, which often means
! an item is missing.
program format_unused
implicit none
integer :: n
real :: x
n = 123
x = 4.5
print "(i6, f8.4)", n            ! expect: format-unused
print "(i6, f8.4)", n, x
end program format_unused

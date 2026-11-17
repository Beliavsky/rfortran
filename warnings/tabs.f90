! Warning:    tabs
! Enabled by: -Wstyle
! A tab character in free-form source. Tabs are not in the Fortran character
! set, and editors show them at different widths. The warning is given once
! per file.
program tabs
implicit none
integer :: n
n = 2
	print *, n           ! expect: tabs
print *, n + 1
end program tabs

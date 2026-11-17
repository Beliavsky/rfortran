! Warning:    character-truncation
! Enabled by: -Wall
! A CHARACTER value longer than the variable it is assigned to; the extra
! characters are silently dropped.
program character_truncation
implicit none
character(len=5) :: code
character(len=:), allocatable :: name
code = "AB12"
print *, code
code = "AB12345"        ! expect: character-truncation
print *, code
name = "a longer value"
print *, name
end program character_truncation

! Warning:    could-be-at
! Enabled by: -Weverything
! trim(s) in an output list, which the Fortran 2023 AT edit descriptor
! replaces: AT writes a CHARACTER value without its trailing blanks. Only
! for compilers that support Fortran 2023; with -std=f2023 the check is
! also in -Wstyle.
program could_be_at
implicit none
character(len=10) :: name
name = "Ada"
print "(a, a, a)", "[", trim(name), "]"      ! expect: could-be-at
print "(a, a, a)", "[", name, "]"
end program could_be_at

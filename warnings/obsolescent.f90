! Warning:    obsolescent
! Enabled by: -Wstyle
! Features the standard declares obsolescent or deleted, such as COMMON,
! EQUIVALENCE, arithmetic IF, computed GO TO, ENTRY, FORALL and
! CHARACTER*n. Modules, IF constructs and SELECT CASE replace them.
program obsolescent
implicit none
integer :: n
character*8 :: name               ! expect: obsolescent
common /counts/ n                 ! expect: obsolescent
character(len=8) :: other
n = 1
name = "a"
other = "b"
print *, n, name, other
end program obsolescent

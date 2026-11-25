! Warning:    unneeded-target
! Enabled by: -Wextra
! TARGET tells the compiler that a pointer may alias the variable, which can
! prevent optimization. A TARGET variable that no pointer is associated with
! does not need the attribute. A variable that is the target of =>, passed
! to C_LOC, or passed to a TARGET or POINTER dummy argument is not reported.
program unneeded_target
implicit none
integer, target :: i                     ! expect: unneeded-target
real, target :: a(3)
real, pointer :: p(:)
i = 3
print *, i
a = [1.0, 2.0, 3.0]
p => a
print *, sum(p)
end program unneeded_target

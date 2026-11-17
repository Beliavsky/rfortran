! Warning:    dead-store
! Enabled by: -Wextra
! An assignment to a local variable that a later assignment overwrites
! before anything reads it, so the first value is never used.
program dead_store
implicit none
integer :: n, m
n = 1                ! expect: dead-store
n = 2
m = 3
print *, n, m
m = m + 1
print *, m
end program dead_store

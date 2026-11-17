! Warning:    memory-leak
! Enabled by: default
! Storage allocated through a local POINTER and never deallocated: it is
! lost when the procedure returns. An ALLOCATABLE is deallocated
! automatically, so use one unless pointer semantics are needed.
program memory_leak
implicit none
call total(5)
call total_allocatable(5)
contains
subroutine total(n)
integer, intent(in) :: n
real, pointer :: work(:)
allocate (work(n))     ! expect: memory-leak
work = 1.0
print *, sum(work)
end subroutine total
subroutine total_allocatable(n)
integer, intent(in) :: n
real, allocatable :: work(:)
allocate (work(n))
work = 1.0
print *, sum(work)
end subroutine total_allocatable
end program memory_leak

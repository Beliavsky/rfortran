! Warning:    could-be-allocatable
! Enabled by: -Wextra
! A POINTER variable that is only allocated, deallocated, nullified, tested
! with ASSOCIATED and used for its values could be ALLOCATABLE, which is
! deallocated automatically and cannot alias another variable. A pointer
! associated with a target, used as a target, or passed to a POINTER dummy
! argument is not reported.
program could_be_allocatable
implicit none
real, pointer :: x(:)                    ! expect: could-be-allocatable
real, pointer :: y(:)
real, target :: t(3) = [1.0, 2.0, 3.0]
nullify (x)
allocate (x(3))
x = 2*t
if (associated(x)) print *, sum(x)
deallocate (x)
y => t
print *, y
call fill(4)
contains
subroutine fill(n)
integer, intent(in) :: n
real, pointer :: w(:)                    ! expect: could-be-allocatable
allocate (w(n))
w = 1.5
print *, w
deallocate (w)
end subroutine fill
end program could_be_allocatable

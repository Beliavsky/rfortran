! Reproducer for util.f90's recursive gen/combos blocker.
! Expected output: 1 2 3
! The recursive call forwards the whole array and omits the OPTIONAL reset flag.
module generator_mod
implicit none
contains
recursive subroutine gen(m, comb, reset)
integer, intent(in) :: m
integer, intent(inout) :: comb(:)
logical, intent(in), optional :: reset

if (present(reset)) then
   if (reset) comb = 0
end if
if (m <= size(comb)) then
   comb(m) = m
   call gen(m+1, comb)
end if
end subroutine gen
end module generator_mod

program test_recursive_optional_array
use generator_mod, only: gen
implicit none
integer, allocatable :: a(:)
integer :: n

n = 3
allocate(a(n))
call gen(1, a, reset=.true.)
if (any(a /= [1,2,3])) stop 1
print *, a
deallocate(a)
end program test_recursive_optional_array


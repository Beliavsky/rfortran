! Intrinsic assignment must allocate a deferred-shape function result.
module alloc_result_assignment_mod
implicit none
contains
pure function copied(ii) result(values)
integer, intent(in) :: ii(:)
integer, allocatable :: values(:)
values = ii
end function copied
end module alloc_result_assignment_mod

program test_allocatable_function_results
use alloc_result_assignment_mod, only: copied
implicit none
print *, copied([2,5])
end program test_allocatable_function_results

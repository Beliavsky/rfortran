! Optional outputs use the same disjoint-section proof as required outputs.
module optional_output_mod
implicit none
contains
pure subroutine update(i, output, total)
integer, intent(in) :: i
real(kind=kind(0d0)), intent(out), optional :: output(:)
real(kind=kind(0d0)), intent(inout), optional :: total(:)
if (present(output)) output = real(i, kind=kind(0d0))
if (present(total)) total = total + real(i, kind=kind(0d0))
end subroutine
end module

program test_optional_outputs
use optional_output_mod, only: update
implicit none
integer :: i
real(kind=kind(0d0)), allocatable :: values(:,:), totals(:,:)
allocate(values(2,9001), totals(2,9001))
totals = 0d0
do concurrent(i=1:size(values,2))
   call update(i, output=values(:,i), total=totals(:,i))
   call update(i, total=totals(:,i))
end do
do i=1,size(values,2)
   if (any(values(:,i) /= real(i,kind=kind(0d0)))) stop 1
   if (any(totals(:,i) /= 2d0*i)) stop 2
end do
print *, 'PASS'
end program

! Whole-array reduction on an assumed-shape dummy must retain its descriptor.
implicit none
integer :: x(2)
x=10
call accumulate(x)
if(any(x/=[13,13])) stop 1
print *,"PASS"
contains
subroutine accumulate(a)
   integer, intent(inout) :: a(:)
   integer :: i
   do concurrent(i=1:2) reduce(+:a)
      a=a+i
   end do
end subroutine
end

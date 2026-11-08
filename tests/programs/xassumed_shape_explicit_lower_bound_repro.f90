! Regression: dummy lower bounds are local; extents come from the actual section.
! This previously leaked @deferred_extent into an explicit-shape check.
implicit none
integer :: x(-3:3)
x=10
call update(x(-3:3:2))
if(any(x(-3:3:2)/=11)) stop 1
if(any(x(-2:2:2)/=10)) stop 2
print *,"PASS"
contains
subroutine update(a)
   integer, intent(inout) :: a(-2:)
   a=a+1
end subroutine
end

! Reduced from DataFrame reindex: a record result with internal binary search.
module reindex_example
implicit none
type index_map
   integer, allocatable :: positions(:)
end type
contains
pure function locate(a, requested) result(mapped)
integer, intent(in) :: a(:), requested(:)
type(index_map) :: mapped
integer :: i
allocate(mapped%positions(size(requested)))
if (size(requested)==0) return
do i=1,size(requested)
   mapped%positions(i)=search(a,requested(i))
end do
contains
pure integer function search(a,x) result(pos)
integer, intent(in) :: a(:),x
integer :: lo,hi,mid
pos=0
lo=1
hi=size(a)
do while (lo<=hi)
   mid=(lo+hi)/2
   if (a(mid)==x) then
      pos=mid
      return
   else if (a(mid)<x) then
      lo=mid+1
   else
      hi=mid-1
   end if
end do
end function
end function
end module
program main
use reindex_example
implicit none
type(index_map) :: mapped
integer, allocatable :: a(:), requested(:)
a=[1,3,4]
requested=[0,1,2,3,4,5]
mapped=locate(a,requested)
print *,mapped%positions
requested=[integer::]
mapped=locate(a,requested)
print *,size(mapped%positions)
a=[integer::]
requested=[1,2]
mapped=locate(a,requested)
print *,mapped%positions
end program

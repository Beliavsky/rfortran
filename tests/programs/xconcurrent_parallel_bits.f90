! Bit intrinsics in parallel loops and SIMPLE/PURE callees.
implicit none
integer :: i, masks(9000)
do concurrent (i=1:9000)
   call make_mask(i,masks(i))
   if (.not.btest(masks(i),modulo(i-1,8))) masks(i)=-1
   if (popcnt(masks(i))/=1) masks(i)=-1
end do
if (any(masks<0) .or. any(masks>255)) stop 1
if (masks(1)/=1 .or. masks(8)/=128 .or. masks(9)/=1) stop 2
print *, 'PASS'
contains
simple subroutine make_mask(i,mask)
   integer, intent(in) :: i
   integer, intent(out) :: mask
   mask=bit_mask(modulo(i-1,8))
end subroutine
simple integer function bit_mask(position) result(mask)
   integer, intent(in) :: position
   mask=ibset(pos=position,i=0)
end function
end

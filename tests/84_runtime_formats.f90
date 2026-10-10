program runtime_formats
implicit none
character(100) :: fmt
character(8) :: part
integer :: i
fmt = "('number=',i4.3)"
write (*,fmt) -7
fmt = "(2i3)"
write (*,fmt) 12,3456
fmt = "(a4,1x,l2,1x,f7.2)"
print fmt, 'ab', .true., 1.25
fmt = "(/,1x,'wall time elapsed(s) = ',f9.3)"
write (*,fmt) 2.5
part = 'i4'
print '(' // trim(part) // ')', 29
fmt = "('100%, isn''t it?')"
write (*,fmt)
fmt = '()'
write (*,fmt)
do i=1,2
   if (i == 1) then
      fmt='(i3)'
   else
      fmt='(i5.3)'
   end if
   write (*,fmt) i
end do
end program runtime_formats

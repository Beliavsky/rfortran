implicit none
integer :: i, repeat, scratch, offset
real :: x(20000), y(20000)
i=99
scratch=-1
offset=10
x=1.0
y=0.0
do repeat=1,3
   do concurrent(i=1:20000) local(scratch) local_init(offset) shared(x,y,repeat) default(none)
      scratch=i*2
      offset=offset+repeat
      y(i)=x(i)+scratch+offset
   end do
end do
print *,y(1),y(20000)
print *,i,scratch,offset
end

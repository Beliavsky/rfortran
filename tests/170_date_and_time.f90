program calendar_fields
implicit none
character(8) :: date
character(10) :: time
character(5) :: zone
integer :: v(8),y,clock,ms,zh,offset,i
call date_and_time(values=v,zone=zone,time=time,date=date)
y=0
clock=0
ms=0
zh=0
do i=1,8
  y=10*y+iachar(date(i:i))-48
end do
do i=1,6
  clock=10*clock+iachar(time(i:i))-48
end do
do i=8,10
  ms=10*ms+iachar(time(i:i))-48
end do
do i=2,5
  zh=10*zh+iachar(zone(i:i))-48
end do
offset=(zh/100)*60+mod(zh,100)
if(zone(1:1)=='-') offset=-offset
print *,y/10000==v(1),mod(y/100,100)==v(2),mod(y,100)==v(3),offset==v(4)
print *,clock/10000==v(5),mod(clock/100,100)==v(6),mod(clock,100)==v(7),ms==v(8)
print *,v(2)>=1 .and. v(2)<=12,v(3)>=1 .and. v(3)<=31,ms>=0 .and. ms<=999
end program calendar_fields

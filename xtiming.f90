program timing
implicit none
real :: cpu
real(8) :: cpu8
integer :: count,rate,maximum
integer(8) :: count8,rate8,maximum8
call cpu_time(time=cpu)
call cpu_time(cpu8)
call system_clock(count_rate=rate,count_max=maximum,count=count)
call system_clock(count8,rate8,maximum8)
print *,cpu>=0.0,cpu8>=0.0_8
print *,count>=0,count<=maximum,rate>0
print *,count8>=0_8,count8<=maximum8,rate8>0_8
end program timing

! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_clock_properties
use util_mod, only:now,exe_name,set_time,time_elapsed,wall_time_elapsed
use kind_mod, only:dp
implicit none
character(19)::stamp
character(1000)::exe
real(dp)::cpu
integer::ticks
stamp=now()
exe=exe_name()
print *,len_trim(stamp)==19,stamp(5:5)=='-',stamp(8:8)=='-',stamp(14:14)==':'
print *,len_trim(exe)>0
call set_time(cpu,ticks)
print *,cpu>=0.0_dp,time_elapsed(cpu)>=0.0_dp,wall_time_elapsed(ticks)>=0.0_dp
end program test_clock_properties

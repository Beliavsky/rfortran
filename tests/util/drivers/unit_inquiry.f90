! Differential driver: exercise the unchanged util_mod get_unit procedure.
program test_unit_inquiry
use util_mod,only:get_unit
implicit none
integer::u,other,connected
call get_unit(u)
print *,u
open(20,file='inquiry-data.txt',status='old',action='read')
call get_unit(connected,'inquiry-data.txt')
call get_unit(other)
print *,connected,other
call get_unit(connected,'./inquiry-data.txt')
print *,connected
close(20)
call get_unit(u,'inquiry-data.txt')
print *,u
call get_unit(u,'missing.txt')
print *,u
end program test_unit_inquiry

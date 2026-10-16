program test_command_argument
implicit none
character(len=1000)::name
integer::length,status
call get_command_argument(number=0,value=name,length=length,status=status)
print *, 'program name available = ',status==0 .and. length>0
call get_command_argument(1,name,length,status)
if(status==0) then
   print *, 'first argument = ',trim(name)
else
   print *, 'no first argument'
end if
end program test_command_argument

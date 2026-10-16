implicit none
character(len=4096)::value
integer::length,status
call get_command_argument(0,value,length,status)
if(status/=0 .or. length<=0 .or. len_trim(value)/=length) stop 1
call get_command_argument(number=999999,value=value,length=length,status=status)
if(status<=0 .or. length/=0 .or. value/=' ') stop 2
call get_command_argument(0,length=length,status=status)
if(status/=0 .or. length<=0) stop 3
call get_command_argument(0)
print *, 'command argument tests passed'
end

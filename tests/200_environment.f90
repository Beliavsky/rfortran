implicit none
character(len=8)::value
integer::length,status
value='stale'
call get_environment_variable('RFORT_TEST_MISSING_71B8E979',value,length,status)
if(status/=1 .or. length/=0 .or. value/=' ') stop 1
call get_environment_variable(name='RFORT_TEST_MISSING_71B8E979',status=status,length=length)
if(status/=1 .or. length/=0) stop 2
call get_environment_variable('RFORT_TEST_MISSING_71B8E979')
print *, 'environment tests passed'
end

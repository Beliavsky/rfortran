implicit none
character(len=4096)::path
character(len=1)::short
integer::status
call getcwd(path,status)
if(status/=0 .or. len_trim(path)==0) stop 1
call getcwd(short,status)
if(status==0 .or. short/=' ') stop 2
call getcwd(path)
if(len_trim(path)==0) stop 3
print *, 'getcwd tests passed'
end

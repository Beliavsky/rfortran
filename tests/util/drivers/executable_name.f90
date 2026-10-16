! Exercise the unchanged utility function and its host-process intrinsics.
program test_executable_name
use util_mod,only:exe_name
implicit none
character(len=1000)::name
name=exe_name()
print *,len_trim(name)>0
end program

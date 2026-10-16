! Minimal reproduction of the GETCWD output in util_mod:exe_name.
program test_getcwd
implicit none
character(len=1000)::path
path=current_directory()
print *,len_trim(path)>0
contains
function current_directory() result(path)
character(len=1000)::path
call getcwd(path)
end function current_directory
end program test_getcwd

! Reduced from util.f90:vec: an allocatable CHARACTER array result.
program test_vec_optional_result
implicit none
write (*,'(100a10)') vec()
write (*,'(100a10)') vec('one')
write (*,'(100a10)') vec('one','two')
contains
function vec(s1,s2) result(str)
character(len=*), intent(in), optional :: s1,s2
character(len=100), allocatable :: str(:)
if (present(s2)) then
   allocate(str(2))
   str(1)=s1
   str(2)=s2
else if (present(s1)) then
   allocate(str(1))
   str(1)=s1
else
   allocate(str(0))
end if
end function vec
end program test_vec_optional_result

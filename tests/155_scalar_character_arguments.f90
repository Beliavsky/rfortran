! Reproduce util.f90:assert_eq2's CHARACTER argument and scalar INTEGER result.
! Expected output: 3 and 4.
program test_assert_eq2
implicit none
print *, assert_eq2(3, 3, 'first check')
print *, assert_eq2(n2=4, string='second check', n1=4)
contains
function assert_eq2(n1, n2, string)
character(len=*), intent(in) :: string
integer, intent(in) :: n1, n2
integer :: assert_eq2
if (n1 == n2) then
   assert_eq2 = n1
else
   write (*,*) 'nrerror: an assert_eq failed with this tag:', string
   stop 'program terminated by assert_eq2'
end if
end function assert_eq2
end program test_assert_eq2

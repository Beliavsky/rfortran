! Original util.f90:vec, preserving its consecutive-argument assumption.
program test_vec
implicit none
write(*,'(100a10)') vec()
write(*,'(100a10)') vec('one')
write(*,'(100a10)') vec('one','two')
write(*,'(100a10)') vec('one','two','three')
write(*,'(100a10)') vec('one','two','three','four')
write(*,'(100a10)') vec('one','two','three','four','five')
write(*,'(100a10)') vec('one','two','three','four','five','six')
write(*,'(100a10)') vec('one','two','three','four','five','six','seven')
write(*,'(100a10)') vec('one','two','three','four','five','six','seven','eight')
write(*,'(100a10)') vec('one','two','three','four','five','six','seven','eight','nine')
write(*,'(100a10)') vec('one','two','three','four','five','six','seven','eight','nine','ten')
contains
function vec(s1,s2,s3,s4,s5,s6,s7,s8,s9,s10) result(str)
! return a 1-D character array of the arguments present, assuming that later arguments are not present if earlier ones are missing
character (len=*), intent(in), optional :: s1,s2,s3,s4,s5,s6,s7,s8,s9,s10
character (len=100), allocatable :: str(:)
if (present(s10)) then
   allocate (str(10))
   str(1)=s1; str(2)=s2; str(3)=s3; str(4)=s4; str(5)=s5; str(6)=s6; str(7)=s7; str(8)=s8; str(9)=s9 ; str(10)=s10
else if (present(s9)) then
   allocate (str(9))
   str(1)=s1; str(2)=s2; str(3)=s3; str(4)=s4; str(5)=s5; str(6)=s6; str(7)=s7; str(8)=s8; str(9)=s9
else if (present(s8)) then
   allocate (str(8))
   str(1)=s1; str(2)=s2; str(3)=s3; str(4)=s4; str(5)=s5; str(6)=s6; str(7)=s7; str(8)=s8
else if (present(s7)) then
   allocate (str(7))
   str(1)=s1; str(2)=s2; str(3)=s3; str(4)=s4; str(5)=s5; str(6)=s6; str(7)=s7
else if (present(s6)) then
   allocate (str(6))
   str(1)=s1; str(2)=s2; str(3)=s3; str(4)=s4; str(5)=s5; str(6)=s6
else if (present(s5)) then
   allocate (str(5))
   str(1)=s1; str(2)=s2; str(3)=s3; str(4)=s4; str(5)=s5
else if (present(s4)) then
   allocate (str(4))
   str(1)=s1; str(2)=s2; str(3)=s3; str(4)=s4
else if (present(s3)) then
   allocate (str(3))
   str(1)=s1; str(2)=s2; str(3)=s3
else if (present(s2)) then
   allocate (str(2))
   str(1)=s1; str(2)=s2
else if (present(s1)) then
   allocate (str(1))
   str(1)=s1
else
   allocate (str(0))
end if
end function vec
end program test_vec

! Reproducer for util.f90: str_real uses internal WRITE in an ELEMENTAL function.
! Expected output:
! 1.2500
! 1.25
program test_str_real
use iso_fortran_env, only: real64
implicit none
print '(a)', trim(str_real(1.25_real64))
print '(a)', trim(str_real(1.25_real64, '(f10.2)'))
contains
elemental function str_real(xx, fmt_real) result(ch)
real(real64), intent(in) :: xx
character(*), intent(in), optional :: fmt_real
character(20) :: ch
if (present(fmt_real)) then
   write(ch, fmt_real) xx
else
   write(ch, '(f20.4)') xx
end if
ch = adjustl(ch)
end function str_real
end program test_str_real

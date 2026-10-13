! Regression for omitted optional arguments in util.f90: str_int(j).
! An unrelated derived type must not make optional arguments required.
! Expected output: 7
program test_str_int
implicit none
type :: unrelated_type
   integer :: value
end type unrelated_type
print '(a)', trim(str_int(7))
contains
elemental function str_int(i, ndigits, prefix) result(ch)
integer, intent(in) :: i
integer, intent(in), optional :: ndigits
character(*), intent(in), optional :: prefix
character(20) :: ch
ch = achar(iachar('0') + i)
if (present(ndigits)) ch = repeat('0', ndigits - 1) // trim(ch)
if (present(prefix)) ch = trim(prefix) // trim(ch)
end function str_int
end program test_str_int

! Regression for the misleading READ diagnostic at xacv.f90:16948.
! No READ statement or vector subscript is used here.
! Expected output: 1 2 3.
! The module name must not change how this ordinary procedure call is handled.
module read_copy_mod
implicit none
contains
subroutine copy_array(a,b)
integer,intent(in)::a(:)
integer,intent(out)::b(:)
b=a
end subroutine
end module

program test_read_named_module_section
use read_copy_mod
implicit none
integer,allocatable::a(:),b(:)
integer::n
n=3
allocate(a(n),b(n))
a=[1,2,3]
call copy_array(a(:n),b)
print *,b
end program

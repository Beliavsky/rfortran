! Reproducer: a local array shadows a procedure named ivec.
! Expected output: 7
program test_ivec_shadow
implicit none
type :: unrelated_type
   integer :: value
end type unrelated_type
integer :: values(1,2)
values = 7
call show(values)
contains
integer function ivec(n) result(j)
integer, intent(in) :: n
j = n
end function ivec
subroutine show(ivec)
integer, intent(in) :: ivec(:,:)
print *, ivec(1,2)
end subroutine show
end program test_ivec_shadow

module optional_setters
implicit none
integer,parameter :: dp=8
contains
elemental subroutine set_optional_real(xx,xdef,xopt)
! set xx to (xopt,xdef) if xopt (is,is not) PRESENT
real(kind=dp), intent(out)          :: xx
real(kind=dp), intent(in)           :: xdef
real(kind=dp), intent(in), optional :: xopt
if (present(xopt)) then
   xx = xopt
else
   xx = xdef
end if
end subroutine set_optional_real
elemental subroutine set_optional_integer(i,idef,iopt)
! set i to (iopt,idef) if iopt (is,is not) PRESENT
integer, intent(out)          :: i
integer, intent(in)           :: idef
integer, intent(in), optional :: iopt
if (present(iopt)) then
   i = iopt
else
   i = idef
end if
end subroutine set_optional_integer
elemental subroutine set_optional_logical(xx,xdef,xopt)
! set xx to (xopt,xdef) if xopt (is,is not) PRESENT
logical, intent(out)          :: xx
logical, intent(in)           :: xdef
logical, intent(in), optional :: xopt
if (present(xopt)) then
   xx = xopt
else
   xx = xdef
end if
end subroutine set_optional_logical
elemental subroutine set_optional_character(xx,xdef,xopt)
! set xx to (xopt,xdef) if xopt (is,is not) PRESENT
character(len=*), intent(out)          :: xx
character(len=*), intent(in)           :: xdef
character(len=*), intent(in), optional :: xopt
if (present(xopt)) then
   xx = xopt
else
   xx = xdef
end if
end subroutine set_optional_character
pure elemental subroutine bump(value,step)
integer,intent(inout)::value
integer,intent(in)::step
value=value+step
end subroutine bump
elemental pure subroutine wrapped(value,default)
integer,intent(out)::value
integer,intent(in)::default
call set_optional_integer(value,default)
end subroutine wrapped
end module optional_setters
program optional_setter_test
use optional_setters
implicit none
real(dp) :: x
integer :: i
logical :: tf
character(5) :: text
character(0) :: empty
call set_optional_real(x,1.25d0)
print '(f6.2)', x
call set_optional_real(x,1.25d0,2.5d0)
print '(f6.2)', x
call set_optional_integer(i,3)
print '(i3)', i
call set_optional_integer(i,3,7)
print '(i3)', i
call set_optional_logical(tf,.false.)
print '(l1)', tf
call set_optional_logical(tf,.false.,.true.)
print '(l1)', tf
call set_optional_character(text,'ab')
print "('[',a,']')", text
call set_optional_character(text,'ab','longer')
print "('[',a,']')", text
call set_optional_character(text,'ab','')
print "('[',a,']')", text
call set_optional_character(empty,'abcdef')
print "('[',a,']')", empty
call bump(i,2)
print '(i3)', i
call wrapped(i,11)
print '(i3)', i
end program optional_setter_test

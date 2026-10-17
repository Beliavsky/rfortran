! Reproduces the allocation-bound blocker in xutil.f90.
module concat_empty_mod
implicit none
interface set_alloc
   module procedure set_alloc_character_vec
end interface set_alloc
contains
pure function concat_char_vec(xx,yy) result(zz)
character(len=*), intent(in) :: xx(:),yy(:)
character(len=max(len(xx),len(yy))) :: zz(size(xx)+size(yy))
integer :: nx
nx=size(xx)
zz(:nx)=xx
zz(nx+1:)=yy
end function concat_char_vec

pure subroutine set_alloc_character_vec(xx,yy,nsize)
character(len=*), intent(in) :: xx(:)
character(len=*), intent(out), allocatable :: yy(:)
integer, intent(out), optional :: nsize
allocate(yy(size(xx)))
yy=xx
if(present(nsize)) nsize=size(yy)
end subroutine set_alloc_character_vec
end module concat_empty_mod

program test_set_alloc_concat_empty
use concat_empty_mod
implicit none
character(len=10), allocatable :: xchar(:),char_vec(:)
allocate(xchar(0))
call set_alloc(concat_char_vec(xchar,['four','five','zero']),char_vec)
print *,size(char_vec),len(char_vec)
print '(a)',trim(char_vec(1))//'|'//trim(char_vec(2))//'|'//trim(char_vec(3))
end program test_set_alloc_concat_empty

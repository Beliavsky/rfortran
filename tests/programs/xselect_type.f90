program test_select_type
implicit none
type base
   integer :: value
end type
type, extends(base) :: child
   integer :: extra
end type
type, extends(child) :: grandchild
   integer :: last
end type
type(base) :: a
type(child) :: b
type(grandchild) :: c
a%value=1
b%value=2
b%extra=3
c%value=4
c%extra=5
c%last=6
call change(a)
call change(b)
call change(c)
print *, a%value, b%value, b%extra, c%value, c%extra, c%last
contains
subroutine change(x)
class(base), intent(inout) :: x
select type (v => x)
class is (child)
   v%extra=v%extra+20
type is (child)
   v%extra=v%extra+10
class default
   v%value=v%value+100
end select
end subroutine
end program

program main
implicit none
integer :: a(2048,3), i
 do i=1,2048
 a(i,1)=i
 a(i,2)=2*i
 a(i,3)=3*i
 end do
call inspect(a(:,2), 'column')
call inspect(a(2048:1:-2,3), 'reverse')
call inspect(a(2:1,1), 'empty')
contains
subroutine inspect(x,label)
integer,intent(in)::x(:)
character(len=*),intent(in),optional::label
if (present(label)) print *,trim(label),size(x),sum(x)
end subroutine
end

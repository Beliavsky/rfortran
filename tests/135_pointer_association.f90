program main
implicit none
real(8),target::a(3,2)
real(8),pointer::p(:)
integer::column
a=reshape([1d0,2d0,3d0,4d0,5d0,6d0],[3,2])
column=2
call get_column(yy=p,k=column,xx=a)
column=1
print *,associated(p),size(p),sum(p),lbound(p),ubound(p)
a(1,2)=7d0
p(2)=8d0
print *,p
print *,a(:,2)
p=p+1d0
print *,a(:,2)
call get_column(a,1,p)
print *,p
call get_column(a,2)
print *,p
nullify(p)
print *,associated(p)
contains
subroutine get_column(xx,k,yy)
real(8),intent(in),target::xx(:,:)
integer,intent(in)::k
real(8),pointer,intent(out),optional::yy(:)
if (present(yy)) yy=>xx(:,k)
end subroutine
end program main

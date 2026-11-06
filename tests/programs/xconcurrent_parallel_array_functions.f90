implicit none
integer :: j
real :: x(4,9000),energy(9000),total(9000)
x=2.0
do concurrent(j=1:9000)
   energy(j)=squared_norm(x(:,j))+4.0
end do
call fill(x,total)
if(any(energy/=20.0) .or. any(total/=8.0)) stop 1
print *,energy(1),energy(9000),total(1),total(9000)
contains
pure real function squared_norm(a) result(r)
real,intent(in)::a(:)
r=dot_product(a,a)
end function
subroutine fill(a,b)
real,intent(in)::a(:,:)
real,intent(out)::b(:)
integer::j
do concurrent(j=1:size(b))
   b(j)=array_sum(a(:,j))
end do
end subroutine
pure real function array_sum(a)
real,intent(in)::a(:)
array_sum=sum(a)
end function
end

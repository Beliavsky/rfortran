program main
implicit none
real(8),allocatable::a(:,:),x(:),y(:)
x=[1.25d0,2.5d0,3.75d0]
y=[4.25d0,5.5d0,6.75d0]
a=cbind(x,y)
print *,shape(a)
print *,a
contains
pure function cbind(x,y) result(a)
real(8),intent(in)::x(:),y(:)
real(8),allocatable::a(:,:)
integer::n
n=size(x)
a=reshape([x,y],[n,2])
end function
end program

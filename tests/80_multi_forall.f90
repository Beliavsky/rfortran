program multi_forall
implicit none
integer :: i,j,k,a(2,3),b(2,2),c(2,2,2),v(6)
i=99
j=98
k=97
forall(i=1:2,j=1:3) a(i,j)=10*j+i
print *, a
forall(i=1:2,j=1:2) b(i,j)=10*i+j
forall(i=1:2,j=1:2) b(i,j)=b(j,i)
print *, b
forall(i=2:1:-1,j=3:1:-1) v(i+2*(j-1))=100*i+j
print *, v
forall(i=1:2,j=1:2,k=1:2) c(i,j,k)=100*i+10*j+k
print *, c
forall(i=1:2,j=1:3) a(i,j)=a(3-i,4-j)
forall(i=1:2,j=2:1) a(i,j)=0
print *, a
forall(i=1:2,j=1:2) c(:,i,j)=c(:,3-i,3-j)
print *, c
print *, i,j,k
call fill(b)
print *, b
contains
pure subroutine fill(x)
integer, intent(out) :: x(:,:)
integer :: i,j
i=90
j=91
forall(i=1:size(x,1),j=1:size(x,2)) x(i,j)=i+j
end subroutine fill
end program multi_forall

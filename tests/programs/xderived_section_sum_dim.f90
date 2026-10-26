module records
implicit none
type :: frame
real(kind=8),allocatable :: values(:,:)
end type
contains
function moving_sum(x,nterms) result(y)
type(frame),intent(in)::x
integer,intent(in)::nterms
type(frame)::y
integer::i,j
y=x
do j=1,size(x%values,1)
i=max(1,j-nterms+1)
y%values(j,:)=sum(x%values(i:j,:),dim=1)
end do
end function
end module
program main
use records
implicit none
type(frame)::x,y
allocate(x%values(5,2))
x%values=reshape([10._8,20._8,30._8,40._8,50._8,1._8,2._8,3._8,4._8,5._8],[5,2])
y=moving_sum(x,3)
print *,y%values
end

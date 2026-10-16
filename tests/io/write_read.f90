program main
implicit none
integer, parameter :: wp = kind(1.0d0), n1 = 10, n2 = 3
real(kind=wp) :: x(n1, n2), xcp(n1, n2)
integer :: i, iu
character (len=100) :: data_file
data_file = "temp.txt"
call random_number(x)
open (newunit=iu, file=data_file, action="write", status="replace")
do i=1,n1
   write (iu, "(*(1x,f0.12))") x(i,:)
end do
close(iu)
open (newunit=iu, file=data_file, action="read", status="old", position="rewind")
do i=1,n1
   read (iu, *) xcp(i,:)
end do
print*,maxval(abs(x-xcp))
end program main
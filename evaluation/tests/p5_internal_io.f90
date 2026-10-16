program p5
implicit none
character(len=20) :: buf
integer :: i, ios
real :: r
write(buf, '(i0,1x,f6.2)') 42, 3.14159
print '(a)', trim(buf)
read(buf, *) i, r
print *, i, r
open(10, file='p5.tmp', status='replace')
write(10,*) 1, 2, 3
close(10)
open(10, file='p5.tmp', status='old')
read(10,*,iostat=ios) i
print *, i, ios
close(10, status='delete')
end program

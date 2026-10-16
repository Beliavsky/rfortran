module utility_driver_regression
interface add_one
module procedure next_integer
end interface
contains
elemental integer function next_integer(x) result(y)
integer,intent(in)::x
y=x+1
end function
subroutine copy_chars(x,y)
character(*),intent(in)::x(:,:)
character(*),allocatable,intent(out)::y(:,:)
allocate(y(size(x,1),size(x,2)))
y=x
end subroutine
end module
program test_utility_driver_regressions
use utility_driver_regression
implicit none
character(2)::words(2),repeated(4)
character(2),allocatable::matrix(:,:)
integer::i,n,u,ios
words=['ab','cd']
n=2
i=99
repeated=[(words,i=1,n)]
if(any(repeated/=['ab','cd','ab','cd']))stop 1
if(i/=99)stop 2
if(any(add_one([1,2])/=[2,3]))stop 3
call copy_chars(reshape(words,[1,2]),matrix)
if(any(shape(matrix)/=[1,2]))stop 4
if(matrix(1,1)/='ab'.or.matrix(1,2)/='cd')stop 5
open(newunit=u,status='scratch',action='readwrite')
write(u,'(a)')'-42'
rewind(u)
read(u,*,iostat=ios)n
if(ios/=0)stop 6
if(n/=-42)stop 7
read(u,*,iostat=ios)n
if(ios>=0)stop 8
close(u)
print *,'utility driver regressions passed'
end program

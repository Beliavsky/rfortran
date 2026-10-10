program constructors
implicit none
integer :: i,j,a(6),b(5),empty(0)
integer, parameter :: n=3
integer, parameter :: p(3)=(/(2*i,i=1,n)/)
i=77
j=88
a=[((10*j+i,i=1,2),j=1,n)]
print *, a
a=(/ (a(i),i=6,1,-1) /)
print *, a
b=(/ 9, (i,i=1,3), 8 /)
print *, b
b=[[(i,i=1,2)],(/3,4/),5]
print *, b
a=[((i+j,i=1,j),j=1,3)]
print *, a
empty=[(i,i=2,1)]
print *, p
print *, i,j
b=(/ (10 / (i+1),i=1,5) /)
print *, b
print '(i0)', checksum()
contains
pure integer function checksum() result(v)
integer :: k,x(3)
x=(/(k*k,k=1,3)/)
v=sum(x)
end function checksum
end program constructors

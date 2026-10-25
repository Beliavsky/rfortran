program xruntime_array_shifts
implicit none
integer :: n
integer, allocatable :: a(:,:), shifts(:), boundary(:), restored(:,:)
logical, allocatable :: mask(:,:)
n=2
allocate(a(n,3), shifts(n), boundary(n), mask(n,3))
a=reshape([1,2,3,4,5,6],[2,3])
shifts=[1,-1]
boundary=[9,8]
print *, cshift(a,1,dim=2)
print *, eoshift(a,shifts,boundary,dim=2)
mask=mod(a,2)==1
restored=unpack([10,20,30],mask,0)
print *, restored
end program

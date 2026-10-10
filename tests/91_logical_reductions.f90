program logical_reductions
implicit none
integer, parameter :: wide=8, axis=1
integer :: a(-2:2), matrix(2,3), n
integer(kind=8) :: nwide
logical :: mask(5), empty(0)
a=[-2,-1,0,1,2]
mask=a>0
matrix=2
print *,count(mask),count(mask=.not.mask),count(a>=0)
print *,count(dim=axis,mask=mask),count(mask,1,4)
nwide=count(kind=wide,mask=mask,dim=1)
print *,nwide,nwide+2147483647
print *,any(mask=mask),all(dim=1,mask=mask)
print *,any(mask,1),all(mask=.not.mask,dim=1)
print *,count(empty),any(empty),all(empty)
print *,count(empty,dim=1,kind=8),any(dim=1,mask=empty),all(empty,1)
print *,count(a(-2:2:2)>0),count(mask(5:1:-1))
print *,count(matrix>1),any(matrix==2),all(matrix>0)
n=positive_count()
print *,n,wide_count()
contains
pure integer function positive_count() result(n)
integer :: x(5)
x=[-2,-1,0,1,2]
n=count(mask=x>0,dim=1)
end function positive_count
pure function wide_count() result(n)
integer :: x(5)
integer(kind=8) :: n
x=[-2,-1,0,1,2]
n=count(mask=x>=0,kind=8)
end function wide_count
end program logical_reductions


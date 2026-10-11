program runtime_allocations
implicit none
integer::iter,n1,n2,ny
real,allocatable::y(:,:)
print *,allocated(y)
do iter=1,3
  if (iter==2) then
    n1=2
    n2=5
  else
    n1=3
    n2=4
  end if
  if (allocated(y)) deallocate(y)
  allocate(y(n1,n2))
  y=2.0
  ny=size(y)
  print *,iter,ny,size(y,1),size(y,2),sum(y),y(n1,n2)
  print *,shape(y),lbound(y),ubound(y)
  print *,sum(y)/y
end do
deallocate(y)
print *,allocated(y)
end program runtime_allocations

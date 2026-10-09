! echo: 2
implicit none
integer :: nset, i
real :: x(3)
read(*,*) nset
do i=1,3
  if (i <= nset) then
    x(i) = 10.0
  end if
end do
print *, sum(x)
end

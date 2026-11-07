! Complex addition and multiplication need both components of every partial.
implicit none
integer :: i,n
integer, parameter :: dp=kind(0d0)
complex(dp) :: total, product
complex(dp), allocatable :: rows(:,:)
n=9001
allocate(rows(-1:0,3:4))
total=(3d0,4d0)
product=(2d0,3d0)
rows=total
do concurrent(i=1:n) reduce(+:total,rows) reduce(*:product)
   total=total+(1d0,-2d0)
   rows=rows+(1d0,-2d0)
   product=product*(0d0,1d0)
end do
if(abs(total-(9004d0,-17998d0))>1d-8) stop 1
if(any(abs(rows-total)>1d-8)) stop 2
if(abs(product-(-3d0,2d0))>1d-8) stop 3
print *, 'PASS'
end

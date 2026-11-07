! Run with: rfort --parallel --parallel-report xconcurrent_parallel_array_intrinsics.f90 --run
implicit none
integer :: i, failures(9000)
integer, allocatable :: a(:)
real(kind=kind(0d0)), allocatable :: x(:)
logical, allocatable :: mask(:)
allocate(a(4),x(4),mask(4))
a = [1,3,5,7]
x = [3d0,4d0,0d0,0d0]
mask = [.true.,.false.,.true.,.false.]
do concurrent(i=1:9000)
   failures(i) = probe(a,x,mask)
end do
if (any(failures /= 0)) error stop 1
print *, "PASS"
contains
pure integer function probe(a,x,mask) result(failures)
   integer, intent(in) :: a(:)
   real(kind=kind(0d0)), intent(in) :: x(:)
   logical, intent(in) :: mask(:)
   failures = 0
   if (iall(a) /= 1) failures = failures+1
   if (iany(a,mask=mask) /= 5) failures = failures+1
   if (iparity(a,mask=mask) /= 4) failures = failures+1
   if (parity(mask)) failures = failures+1
   if (abs(norm2(x)-5d0) > 1d-12) failures = failures+1
   if (minloc(x,dim=1,back=.true.) /= 4) failures = failures+1
   if (maxloc(x,dim=1) /= 2) failures = failures+1
   if (findloc(x,0d0,dim=1) /= 3) failures = failures+1
end function
end

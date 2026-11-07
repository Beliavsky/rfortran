! Each worker owns work/weights storage; locality is reapplied each iteration.
program test_parallel_local_arrays
implicit none
integer :: i
real(kind=kind(0d0)) :: work(2), weights(2), a(9001)
weights = [1d0, 2d0]
do concurrent(i=1:size(a)) local(work) local_init(weights) shared(a) default(none)
   work(1) = real(i, kind=kind(0d0))
   work(2) = 2d0*i
   weights(1) = weights(1) + real(i, kind=kind(0d0))
   a(i) = sum(work) + sum(weights)
end do
do i=1,size(a)
   if (a(i) /= 4d0*i+3d0) stop 1
end do
if (any(weights /= [1d0,2d0])) stop 2
print *, 'PASS'
end program

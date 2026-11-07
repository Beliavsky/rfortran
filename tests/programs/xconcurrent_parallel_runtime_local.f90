program test_runtime_local_arrays
implicit none
integer :: n, i, a(9001)
do n=1,4
   call compute(n, a)
   do i=1,size(a)
      if (a(i) /= (n+1)*i+2*n) stop 1
   end do
end do
print *, 'PASS'
contains
subroutine compute(n, a)
integer, intent(in) :: n
integer, intent(out) :: a(:)
integer :: i, work(n), weights(n)
weights = 2
do concurrent(i=1:size(a)) local(work) local_init(weights)
   work = i
   weights(1) = weights(1) + i
   a(i) = sum(work) + sum(weights)
end do
if (any(weights /= 2)) stop 2
end subroutine
end program

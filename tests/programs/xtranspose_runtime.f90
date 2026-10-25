program xtranspose_runtime
implicit none
integer :: n
real, allocatable :: a(:,:)
n = 2
allocate(a(n,3))
a = reshape([1.25,2.5,3.75,4.0,5.5,6.0],[2,3])
call show(transpose(a))
contains
subroutine show(x)
real, intent(in) :: x(:,:)
print *, shape(x)
print *, x
end subroutine show
end program xtranspose_runtime

program q1
implicit none
integer :: n, i
real(8), allocatable :: x(:)
character(len=32) :: arg
call get_command_argument(1, arg)
read(arg, *) n
allocate(x(n))
do i = 1, n
  x(i) = 1.0d0/i
end do
print *, n, sum(x)
call show(x)
contains
subroutine show(v)
  real(8), intent(in) :: v(:)
  print *, size(v), maxval(v), minval(v)
end subroutine
end program

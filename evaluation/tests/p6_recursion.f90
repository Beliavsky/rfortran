program p6
implicit none
integer, allocatable :: a(:)
integer :: i
a = [5, 3, 9, 1, 7, 2, 8]
call qsort(a)
print *, a
print *, fib(20)
contains
recursive subroutine qsort(v)
  integer, intent(inout) :: v(:)
  integer :: p
  if (size(v) <= 1) return
  p = v(1)
  v = [pack(v(2:), v(2:) < p), p, pack(v(2:), v(2:) >= p)]
  i = count(v < p)
  call qsort(v(1:i))
  call qsort(v(i+2:))
end subroutine
recursive integer function fib(n) result(f)
  integer, intent(in) :: n
  if (n < 2) then
    f = n
  else
    f = fib(n-1) + fib(n-2)
  end if
end function
end program

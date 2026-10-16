program p7
implicit none
character(len=:), allocatable :: s, t
integer :: k
s = "hello, world"
t = upper(s)
print '(a)', t
k = index(s, ",")
print '(a,"|",a)', s(1:k-1), adjustl(s(k+1:))
print *, len_trim("ab  "), repeat("xy", 3)
contains
function upper(x) result(y)
  character(len=*), intent(in) :: x
  character(len=len(x)) :: y
  integer :: i
  y = x
  do i = 1, len(x)
    if (x(i:i) >= 'a' .and. x(i:i) <= 'z') y(i:i) = achar(iachar(x(i:i)) - 32)
  end do
end function
end program

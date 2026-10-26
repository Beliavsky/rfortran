program allocatable_scalar_extensions
implicit none
integer :: n
integer, allocatable :: value
character(:), allocatable :: text

n = 6
value = factorial(n)
text = make_text(n)
print *, value, len(text), text

contains

pure recursive function factorial(n) result(value)
integer, intent(in) :: n
integer, allocatable :: value
if (n <= 1) then
   value = 1
else
   value = n*factorial(n-1)
end if
end function factorial

function make_text(n) result(text)
integer, intent(in) :: n
character(len=n), allocatable :: text
text = repeat('x', n)
end function make_text

end program allocatable_scalar_extensions

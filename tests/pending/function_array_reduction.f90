! Known failure, excluded from the passing numbered fixtures.
! Expected output: 15. Both interpreter and native paths need array-dummy fixes.
integer :: a(5)
a=[1,2,3,4,5]
print *,array_sum(a)
contains
pure integer function array_sum(x) result(n)
integer,intent(in) :: x(5)
n=sum(x)
end function array_sum
end


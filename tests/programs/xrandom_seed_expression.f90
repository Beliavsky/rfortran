! RANDOM_SEED PUT is input: an array expression is a valid actual.
! Expected output: T
implicit none
integer :: n
integer, allocatable :: seed(:)
real :: a(4), b(4)
call random_seed(size=n)
allocate(seed(n))
seed=123
call random_seed(put=seed+37)
call random_number(a)
seed=seed+37
call random_seed(put=seed)
call random_number(b)
print *,all(a==b)
end

! RANDOM_SEED keyword forms must save and restore the random stream.
! Expected output: T T T.
implicit none
integer :: n
integer, allocatable :: seed(:), saved(:)
real :: a(3), b(3)
call random_seed(size=n)
allocate(seed(n),saved(n))
seed = 12345
call random_seed(put=seed)
call random_seed(get=saved)
call random_number(harvest=a)
call random_seed(put=saved)
call random_number(harvest=b)
print *, all(a==b), all(a>=0.0 .and. a<1.0), n>0
end

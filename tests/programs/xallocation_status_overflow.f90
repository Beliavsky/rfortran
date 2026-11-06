implicit none
integer, allocatable :: a(:,:), b(:)
integer :: s
integer(kind=8) :: n, lo, hi
character(len=40) :: message
n=huge(n)/2
allocate(a(n,n),stat=s,errmsg=message)
print *,s/=0,allocated(a),len_trim(message)>0
allocate(a(0,n),stat=s)
print *,s==0,allocated(a),size(a)
deallocate(a)
allocate(b(n),stat=s)
print *,s/=0,allocated(b)
lo=-huge(lo)
hi=huge(hi)
allocate(b(lo:hi),stat=s)
print *,s/=0,allocated(b)
allocate(b(3),source=7,stat=s)
print *,s==0,b
deallocate(b)
end

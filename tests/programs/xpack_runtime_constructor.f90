implicit none
logical,allocatable::remove(:)
integer,allocatable::keep(:)
integer::i,n
n=5
allocate(remove(n))
remove=[.true.,.false.,.true.,.false.,.false.]
keep=pack([(i,i=1,size(remove))],.not.remove)
print *,size(keep),keep
remove=.true.
keep=pack([(i,i=1,size(remove))],.not.remove)
print *,size(keep)
deallocate(remove)
allocate(remove(0))
remove=.false.
keep=pack([(i,i=1,size(remove))],.not.remove)
print *,size(keep)
end

program test_partition
implicit none
integer :: n, npart
n = 10
npart = 3
print *, partition(n, npart)
print *, partition(1, npart)
print *, partition(2, 4)
print *, size(partition(10, 0))
contains
pure function partition(n,npart) result(ipart)
integer, intent(in) :: n,npart
integer :: ipart(npart),i
if (npart <= 0) return
if (n <= 1) then
   ipart = n
   return
end if
if (npart < n) then
   forall (i=1:npart) ipart(i) = nint(n*i/(npart+1.0d0))
else
   forall (i=1:npart) ipart(i) = i
end if
end function
end program

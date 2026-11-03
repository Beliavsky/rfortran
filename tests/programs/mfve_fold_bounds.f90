! Reduced from C:/fortran/2015/util.f90: fold_bounds and group_sizes.
! Compile this module separately (-c) to reproduce the nfolds diagnostic.
module fve_fold_bounds_repro
implicit none
contains
subroutine fold_sizes(n,nfolds,sizes)
integer, intent(in) :: n,nfolds
integer, intent(out) :: sizes(nfolds)
integer :: nelem(nfolds)
nelem = group_sizes(n,nfolds)
sizes = nelem
end subroutine fold_sizes

recursive pure function group_sizes(n,ngroups) result(nelem)
integer, intent(in) :: n,ngroups
integer :: nelem(ngroups),ndiv
if (ngroups < 1) then
   return
else if (ngroups == 1) then
   nelem = max(0,n)
   return
else if (ngroups > n) then
   nelem(1:n) = 1
   nelem(n+1:) = 0
   return
end if
ndiv = n/ngroups
nelem = ndiv + group_sizes(n-ndiv*ngroups,ngroups)
end function group_sizes
end module fve_fold_bounds_repro

! Reduced from statistics.f90:1082 in C:/fortran/2015.
! The unrelated saved and allocated derived objects trigger early constant folding.
! REAL's KIND keyword must be associated before its kind value is evaluated.
module kind_mod
implicit none
integer,parameter::dp=selected_real_kind(15,307)
end module
module bin_prob_result_mod
use kind_mod,only:dp
implicit none
type :: unrelated_type
 real :: value
end type
contains
pure function bin_freq(xthresh,xx) result(nfreq)
! return the frequencies of observations of xx(:) in bins defined by xthresh(:)
real(kind=dp), intent(in) :: xthresh(:)
real(kind=dp), intent(in) :: xx(:)
integer                   :: nfreq(size(xthresh)+1)
integer                   :: i,n,nthresh
n       = size(xx)
nthresh = size(xthresh)
if (nthresh == 0) then
   nfreq = n
   return
end if
nfreq(1) = count(xx<xthresh(1))
do i=1,nthresh-1
   nfreq(i+1) = count(xx>=xthresh(i) .and. xx<xthresh(i+1))
end do
nfreq(nthresh+1) = count(xx>=xthresh(nthresh))
end function  bin_freq
!
pure function bin_prob(xthresh,xx) result(prob)
! return the frequencies of observations of xx(:) in bins defined by xthresh(:)
real(kind=dp), intent(in) :: xthresh(:)
real(kind=dp), intent(in) :: xx(:)
real(kind=dp)             :: prob(size(xthresh)+1)
prob = bin_freq(xthresh,xx)/real(max(1,size(xx)),kind=dp)
end function bin_prob
end module
program main
use bin_prob_result_mod
implicit none
type(unrelated_type),allocatable,target::obj
type(unrelated_type),save::saved=unrelated_type(0.)
allocate(obj)
obj%value=1.0
print *,obj%value
print '(3f8.4)',bin_prob([2._dp,4._dp],[1._dp,2._dp,4._dp,5._dp])
end

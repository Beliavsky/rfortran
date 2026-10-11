module m
implicit none
contains
pure recursive subroutine set_alloc_int_vec(ii,jj,jmin,jmax)
! for integer vectors xx(:) and yy(:), allocate yy and set it to xx
! keep only values between jmin and jmax, inclusive, if they are present
integer, intent(in)               :: ii(:)
integer, intent(out), allocatable :: jj(:)
integer, intent(in) , optional    :: jmin,jmax
integer                           :: ni
if (present(jmin) .and. present(jmax)) then
   call set_alloc_int_vec(pack(ii,ii >= jmin .and. ii <= jmax),jj)
else if (present(jmin)) then
   call set_alloc_int_vec(pack(ii,ii >= jmin),jj)
else if (present(jmax)) then
   call set_alloc_int_vec(pack(ii,ii <= jmax),jj)
else
   ni = size(ii)
   allocate (jj(ni))
   jj = ii
end if
end subroutine set_alloc_int_vec
end module m

program test_pure_recursive
use m, only: set_alloc_int_vec
implicit none
integer, parameter :: ii(6) = [-3, 0, 2, 2, 5, 9]
integer, allocatable :: jj(:)

call set_alloc_int_vec(ii,jj)
call check(ii,"copy all values")

call set_alloc_int_vec(ii,jj,jmin=2)
call check([2,2,5,9],"inclusive lower bound")

call set_alloc_int_vec(ii,jj,jmax=2)
call check([-3,0,2,2],"inclusive upper bound")

call set_alloc_int_vec(ii,jj,jmin=2,jmax=5)
call check([2,2,5],"both bounds, preserving duplicates and order")

call set_alloc_int_vec(ii,jj,jmax=-4)
call check(ii(:0),"no matching values")

call set_alloc_int_vec(ii,jj,jmin=5,jmax=2)
call check(ii(:0),"reversed bounds")

call set_alloc_int_vec(ii(:0),jj)
call check(ii(:0),"empty input")

contains

subroutine check(expected,label)
integer, intent(in) :: expected(:)
character(*), intent(in) :: label
if (.not. allocated(jj)) then
   print '(a)', "FAIL: "//label//": output is not allocated"
else if (size(jj) /= size(expected)) then
   print '(a)', "FAIL: "//label//": incorrect output size"
else if (any(jj /= expected)) then
   print '(a)', "FAIL: "//label//": incorrect output values"
else
   print '(a)', "PASS: "//label
   return
end if
error stop "set_alloc_int_vec test failed"
end subroutine check

end program test_pure_recursive

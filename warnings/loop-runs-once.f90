! Warning:    loop-runs-once
! Enabled by: -Wall
! A DO loop whose body ends in EXIT, RETURN or STOP on every path, so it can
! never start a second iteration; usually the EXIT belongs inside an IF.
program loop_runs_once
implicit none
integer :: i, found
integer :: a(5) = [3, 8, 1, 9, 4]
found = 0
do i = 1, size(a)          ! expect: loop-runs-once
   if (a(i) > 5) found = i
   exit
end do
print *, found
do i = 1, size(a)
   if (a(i) > 5) then
      found = i
      exit
   end if
end do
print *, found
end program loop_runs_once

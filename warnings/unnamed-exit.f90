! Warning:    unnamed-exit
! Enabled by: -Wstyle
! EXIT or CYCLE without a construct name. With nested loops it takes a
! careful reader to see which loop it leaves; naming the loop makes it
! explicit. The warning is given once per loop.
program unnamed_exit
implicit none
integer :: i, j
do i = 1, 3
   do j = 1, 3
      if (j > i) exit          ! expect: unnamed-exit
      print *, i, j
   end do
end do
rows: do i = 1, 3
   cols: do j = 1, 3
      if (j > i) exit cols
      print *, i, j
   end do cols
end do rows
end program unnamed_exit

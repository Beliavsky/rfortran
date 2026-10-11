program named_loops
implicit none
integer :: i, j, k, n
n = 0
do_i: do i = 1, 5
   do j = 1, 3
      do k = 1, 3
         n = n + 1
         if (i == 2) cycle do_i
         if (i == 4) exit do_i
         exit
      end do
   end do
   n = n + 10
end do do_i
print *, i, j, k, n
n = 0
outer: do i = 5, 1, -2
   inner: do j = 1, 4, 2
      n = n + 1
      if (i == 3) cycle outer
      if (j == 1) cycle inner
      exit inner
   enddo inner
enddo outer
print *, i, j, n
end program named_loops

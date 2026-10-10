program nonadvancing_output
implicit none
character(40) :: fmt
character(*), parameter :: keep_record = 'NO'
integer :: i
fmt = '(a)'
write (*,fmt,advance=keep_record) 'left'
write (*,'(a)',advance='n'//'o') ':'
write (*,'(i3)',advance='yes') 7
write (*,"('empty')",advance='no')
write (*,'(a)') '-end'
write (*,'(a)',advance='no') 'loop:'
do i=1,3
   write (*,'(i2)',advance='no') i
end do
write (*,'()')
write (*,'(a)',advance='no') 'mixed:'
print *, 'done'
write (*,'(a)',advance='no') 'slash:'
write (*,"(/,'next')",advance='no')
write (*,'(a)') '-end'
write (*,'(a)',advance='no') 'final'
end program nonadvancing_output

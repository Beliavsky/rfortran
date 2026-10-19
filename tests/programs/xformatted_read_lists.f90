! Input: tests/io/formatted_read_lists.txt
program formatted_read_lists
implicit none
integer :: a(2,3),i,status
a=0
read(*,'(4i3)',iostat=status)a(:,3:2:-1)
if(status/=0) error stop 'section input failed'
read(*,'(2i3)',iostat=status)(a(i,1),i=2,1,-1)
if(status/=0) error stop 'implied-DO input failed'
print '(6i3)',a
print *,i
end program formatted_read_lists

! echo: "abcdef"
! echo: "ignored"
! echo: "xy"
program record_read_iostat
implicit none
character(5) :: text
character(0) :: empty
integer :: status, in_unit
integer(8) :: status8
read(*,'(a)',iostat=status) text
if (status == 0) print '(a)', '['//text//']'
print *, status == 0
read(5,'(a)',IOSTAT=status8) empty
if (status8 == 0) print *, len(empty), status8 == 0
in_unit = 5
status = 99
read(in_unit,'(a)',iostat=status) text
if (status == 0) print '(a)', '['//text//']'
print *, status == 0
read(in_unit,'(a)',iostat=status) text
print *, status < 0
end program record_read_iostat

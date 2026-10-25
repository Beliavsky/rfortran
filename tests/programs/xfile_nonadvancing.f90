program main
implicit none
integer :: unit, ios
character(80) :: line
open(newunit=unit, status='scratch', action='readwrite')
write(unit,'(a)',advance='no') ''
write(unit,'(",",a)',advance='no') 'SPY'
write(unit,'(",",a)',advance='no') 'TLT'
write(unit,*)
write(unit,'(i10)',advance='no') 20200101
write(unit,'(",",f10.4)',advance='no') 100.5
write(unit,'(",",f10.4)',advance='no') 120.25
write(unit,*)
rewind(unit)
do
   read(unit,'(a)',iostat=ios) line
   if (ios /= 0) exit
   print '(a)', trim(line)
end do
close(unit)
end program main

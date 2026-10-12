program main
integer::fileunit
open(newunit=fileunit,status='scratch',action='readwrite')
write(fileunit,'(a)') 'first'
write(fileunit,'(a)') ''
write(fileunit,'(a)') 'last'
rewind(fileunit)
print *,num_records(fileunit)
close(fileunit)
contains
integer function num_records(unit)
integer,intent(in)::unit
num_records=0
do
read(unit=unit,fmt=*,end=1)
num_records=num_records+1
end do
1 continue
end function num_records
end program main

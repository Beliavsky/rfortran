! Fortran 2023 section 9.3: enums use integer values for formatted I/O.
program enum_io
implicit none
enum, bind(c) :: season
   enumerator :: spring=5, summer=7, autumn, winter
end enum
type(season) :: values(3), copy(3)
integer :: unit, status
character(12) :: fmt
values=[season :: spring, season(6), summer]
fmt='(3i3)'
print fmt, values
print '(i3,1x,b4,1x,o2,1x,z2,1x,g0)',values(2),values(2),values(2),values(2),values(2)
open(newunit=unit,status='scratch',action='readwrite')
write(unit,'(3z8.8)')values
rewind(unit)
read(unit,'(3z8)',iostat=status)copy
close(unit)
if(status/=0) error stop 'enum read failed'
print fmt, copy
end program enum_io

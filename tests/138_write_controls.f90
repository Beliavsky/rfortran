program write_controls
implicit none
character(20) :: fmt
fmt = '(a,1x,i0)'
write(*,fmt=fmt) 'value',7
write(fmt=trim(fmt),unit=6) 'next',8
write(advance='no',fmt='(a)',unit=6) 'joined'
write(6,fmt='(a)') 'end'
write(unit=*,fmt=*) 9
write(unit=*,fmt=*)
end program write_controls

program main
implicit none
integer::iunit,ios
integer(kind=8)::wide_status
character(len=8)::text
open(newunit=iunit,status='scratch')
write(iunit,'(a)') 'discard'
write(iunit,'(a)')
write(iunit,'(a)') 'kept'
rewind(iunit)
read(iunit,*)
if (.true.) read(iunit,*); read(iunit,'(a)') text
print *,trim(text)
backspace(iunit)
read(iunit,*,iostat=ios)
print *,ios==0
read(iunit,*,iostat=ios)
print *,ios<0
rewind(iunit)
read(iunit,*,iostat=wide_status)
print *,wide_status==0
close(iunit)
read(iunit,*,iostat=ios)
print *,ios>0
end program main

program file_positioning
implicit none
integer :: iu
character(len=5) :: short
character(len=12) :: text
iu=20
open(unit=iu,file='tests/io/file_records.txt',status='old',action='read')
backspace(iu)
read(iu,'(a)') short
print *, '['//short//']'
read(iu,'(a)') short
print *, '['//short//']'
backspace iu
read(iu,'(a)') short
print *, '['//short//']'
read(iu,'(a)') short
print *, '['//short//']'
backspace(unit=iu)
read(iu,'(a)') text
print *, '['//trim(text)//']'
backspace(iu)
backspace(iu)
read(iu,'(a)') short
print *, '['//short//']'
read(iu,'(a)') text
read(iu,'(a)') text
print *, '['//trim(text)//']'
call reread(iu)
rewind iu
open(21,file='tests/io/file_records.txt',action='read',status='old',access='sequential',form='formatted')
read(21,'(a)') short
print *, '['//short//']'
read(iu,'(a)') short
print *, '['//short//']'
if (.true.) rewind(unit=21)
close(iu)
close(unit=21)
close(21)
open(20,file='tests/io/file_records.txt',status='old',action='read')
read(20,'(a)') short
print *, '['//short//']'
close(20)
contains
subroutine reread(unit_number)
integer,intent(in) :: unit_number
character(len=12) :: line
backspace(unit_number)
read(unit_number,'(a)') line
print *, '['//trim(line)//']'
end subroutine reread
end program file_positioning

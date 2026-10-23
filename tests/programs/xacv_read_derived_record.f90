! xacv.f90 line 2124: internal list-directed READ after skipped fields.
module read_date_mod
implicit none
type :: date_mdy
integer :: month,day,year
end type
contains
subroutine read_date(text,nskip,date)
character(len=*),intent(in)::text
integer,intent(in)::nskip
type(date_mdy),intent(out)::date
character(len=1)::dummy
integer::i,status
read(text,*,iostat=status) (dummy,i=1,nskip),date
if(status/=0) error stop 'read failed'
end subroutine
end module
program test
use read_date_mod
implicit none
type(date_mdy)::date
call read_date('a b 1 2 2024',2,date)
print *,date%month,date%day,date%year
! Expected: 1 2 2024
end program

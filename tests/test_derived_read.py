"""Default derived-type input in interpreter and native O0/O2 builds."""
import unittest
import test_deallocate
from test_record_read import ROOT


class DerivedRead(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_xacv_reproducer(self):
        self.check((ROOT/'tests/programs/xacv_read_derived_record.f90').read_text(), '1 2 2024\n')

    def test_multiple_records_in_one_session(self):
        self.check('''program test
implicit none
type :: pair
integer :: x,y
end type
type(pair)::a,b
integer::first,last,status
character(len=80)::text
text='9 1 2 3 4 8'
read(text,*,iostat=status) first,a,b,last
print *,status,first,a,b,last
end
''', '0 9 1 2 3 4 8\n')

    def test_nested_and_fixed_array_components(self):
        self.check('''program test
implicit none
type :: pair
integer::x,y
end type
type :: record
integer::first
type(pair)::pairs(2)
real::values(2)
logical::flag
character(len=3)::name
end type
type(record)::a
integer::status
character(len=80)::text
text='1 2 3 4 5 6.5 7.5 T abc'
read(text,*,iostat=status) a
print *,status,a
end
''', '0 1 2 3 4 5 6.5 7.5 T abc\n')

    def test_formatted_internal_read(self):
        self.check('''program test
implicit none
type :: record
integer::x
real::y
logical::flag
character(len=3)::name
end type
type(record)::a
integer::status
character(len=20)::text
text='12 3.50Tabc'
read(text,'(i2,f5.2,l1,a3)',iostat=status) a
print *,status,a
end
''', '0 12 3.5 T abc\n')

    def test_stdin_list_read(self):
        self.check('''program test
implicit none
type :: pair
integer::x,y
end type
type(pair)::a,b
read *,a,b
print *,a,b
end
''', '1 2 3 4\n', data='1 2\n3 4\n')

    def test_external_formatted_read(self):
        self.check('''program test
implicit none
type :: pair
integer::x,y
end type
type(pair)::a
integer::status
read(*,'(2i3)',iostat=status) a
print *,status,a
end
''', '0 1 2\n', data='  1  2\n')

    def test_null_repetition_and_slash(self):
        self.check('''program test
implicit none
type :: record
integer::x,y,z
end type
type(record)::a,b
integer::status
character(len=80)::text
a=record(1,2,3)
b=record(4,5,6)
text=',2*9,/'
read(text,*,iostat=status) a,b
print *,status,a,b
end
''', '0 1 9 9 4 5 6\n')

    def test_iostat_reports_input_errors(self):
        self.check('''program test
implicit none
type :: record
integer::x,y,z
end type
type(record)::a
integer::status
character(len=80)::text
a=record(1,2,3)
text='8 bad 9'
read(text,*,iostat=status) a
! Input-list variables become undefined on an input error.
print *,status>0
end
''', 'T\n')

    def test_record_inside_runtime_implied_do(self):
        self.check('''program test
implicit none
type :: pair
integer::x,y
end type
type(pair)::a
integer::i,n,status
character(len=80)::text
text='2 1 2 3 4'
read(text,*,iostat=status) n,(a,i=1,n)
print *,status,a,i
end
''', '0 3 4 3\n')

    def test_formatted_without_status(self):
        self.check('''program test
implicit none
type :: pair
integer::x,y
end type
type(pair)::a
character(len=20)::text
text='  7  8'
read(text,'(2i3)') a
print *,a
end
''', '7 8\n')

    def test_reject_allocatable_components(self):
        self.reject('''program test
implicit none
type :: record
integer,allocatable::x(:)
end type
type(record)::a
character(len=20)::text
text='1 2'
read(text,*) a
end
''', 'input of derived object `a` with an ALLOCATABLE or POINTER component requires defined input/output')


    def test_elements_sections_and_arrays_of_records(self):
        # Elements, sections and whole arrays of records, and elements of an
        # ALLOCATABLE array component of a record, were rejected as input
        # items: "type `port` has no binding `dates`", or "unsupported
        # expression in numeric array reduction".
        self.check('''module m
implicit none
type date
   integer :: month = 0, day = 0, year = 0
end type
type port
   type(date) :: date_exp
   type(date), allocatable :: dates(:)
   real, allocatable :: rates(:), vols(:,:)
end type
contains
subroutine load(xx, n)
type(port), intent(inout) :: xx
integer, intent(in) :: n
character(len=100) :: text
integer :: i
allocate(xx%dates(n), xx%rates(n), xx%vols(n, 2))
do i = 1, n
   write (text, '(i0,",",i0,",",i0,",12,31,2030,",f4.1,",1.5,2.5")') i, 10+i, 2020+i, 0.5*i
   read (text, *) xx%dates(i), xx%date_exp, xx%rates(i), xx%vols(i,:)
end do
end subroutine
end module
program main
use m
implicit none
type(port) :: p
integer :: i
call load(p, 3)
do i = 1, 3
   print *, p%dates(i), p%rates(i), p%vols(i,:)
end do
print *, p%date_exp
end program
''', "1 11 2021 0.5 1.5 2.5\n2 12 2022 1.0 1.5 2.5\n3 13 2023 1.5 1.5 2.5\n12 31 2030\n")
        self.check('''program p
implicit none
type date
   integer :: month, day, year
end type
type rec
   type(date) :: d
   real :: v(2)
end type
type(date) :: a(4)
type(rec) :: r(2)
character(len=60) :: text
a = date(0, 0, 0)
text = '01/02/2003 04/05/2006'
read (text, '(2(i2,1x,i2,1x,i4,1x))') a(2:3)
print *, a
read (*, *) r(2), a(4)
print *, r(2)%d, r(2)%v, a(4)
read (*, *) r
print *, r
end program
''', "0 0 0 1 2 2003 4 5 2006 0 0 0\n7 8 2009 1.5 2.5 9 9 1999\n1 1 2001 0.5 0.25 2 2 2002 3.5 4.5\n", data="7 8 2009 1.5 2.5 9 9 1999\n1 1 2001 0.5 0.25 2 2 2002 3.5 4.5\n")

if __name__ == '__main__':
    unittest.main()

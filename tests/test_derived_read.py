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


if __name__ == '__main__':
    unittest.main()

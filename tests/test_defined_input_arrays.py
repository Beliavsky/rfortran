"""Defined array input: interpreter, native O0/O2, and gfortran."""
import unittest
from test_record_read import ROOT
import test_deallocate

class DefinedInputArrays(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def source(self, declarations, body, data='1 2 3 4 5 6 7 8 9 10 11 12'):
        module=(ROOT/'tests/programs/xdefined_formatted_input.f90').read_text().split('program main')[0]
        return module+f"""program main
use defined_input
implicit none
integer :: u,status
character(100) :: message
{declarations}
open(newunit=u,status='scratch')
write(u,'(a)') '{data}'
rewind(u)
{body}
close(u)
end program
"""

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xdefined_formatted_input_arrays.f90').read_text(),
            '9 11 5 7 1 3\n10 12 6 8 2 4\n')

    def test_array_component(self):
        source=self.source('type(holder) :: w',
            'read(u,*) w%values\nprint *,w%values(1)%x,w%values(2)%x')
        source=source.replace('contains\nsubroutine read_point',
            'type :: holder\ntype(point) :: values(2)\nend type\ncontains\nsubroutine read_point')
        self.check(source,'1 3\n')

    def test_array_followed_by_scalar(self):
        self.check(self.source('type(point) :: a(2),b',
            'read(u,*) a,b\nprint *,a%x,b%x,b%y'),'1 3 5 6\n')

    def test_whole_array(self):
        self.check(self.source('type(point) :: a(3)',
            'read(u,*) a\nprint *,a%x\nprint *,a%y'),'1 3 5\n2 4 6\n')

    def test_element(self):
        self.check(self.source('type(point) :: a(-1:2)',
            'read(u,*) a(0)\nprint *,a(0)%x,a(0)%y'),'1 2\n')

    def test_matrix_order(self):
        self.check(self.source('type(point) :: a(2,3)',
            'read(u,*) a\nprint *,a%x\nprint *,a%y'),'1 3 5 7 9 11\n2 4 6 8 10 12\n')

    def test_rank_three(self):
        self.check(self.source('type(point) :: a(2,1,3)',
            'read(u,*) a\nprint *,a%x\nprint *,a%y'),'1 3 5 7 9 11\n2 4 6 8 10 12\n')

    def test_strided_section(self):
        self.check(self.source('type(point) :: a(-1:3)',
            'a%x=-1\na%y=-1\nread(u,*) a(3:-1:-2)\nprint *,a%x\nprint *,a%y'),
            '5 -1 3 -1 1\n6 -1 4 -1 2\n')

    def test_mixed_scalar_section(self):
        self.check(self.source('type(point) :: a(2,3)',
            'a%x=-1\na%y=-1\nread(u,*) a(2,3:1:-1)\nprint *,a%x'),'-1 5 -1 3 -1 1\n')

    def test_empty_array_then_scalar(self):
        self.check(self.source('type(point) :: a(0),b',
            'read(u,*) a,b\nprint *,b%x,b%y'),'1 2\n')

    def test_error_stops_array(self):
        source=self.source('type(point) :: a(3)',
            "a%x=-1\na%y=-1\nread(u,*,iostat=status,iomsg=message) a\nprint *,status>0,a(1)%x,a(1)%y,a(3)%x,a(3)%y,len_trim(message)>0",'1 2 bad 4 5 6')
        # Local gfortran returns a non-positive parent status for malformed data.
        self.check(source,'T 1 2 -1 -1 T\n',reference=False)

    def test_runtime_shape(self):
        source=self.source('type(point) :: a(3)',
            'call fill(u,a)\nprint *,a%x\nprint *,a%y').replace('close(u)\nend program',"""close(u)
contains
subroutine fill(unit,values)
integer,intent(in) :: unit
type(point),intent(out) :: values(:)
read(unit,*) values
end subroutine
end program""")
        self.check(source,'1 3 5\n2 4 6\n')

    def test_assumed_shape_matrix_section(self):
        source=self.source('type(point) :: a(2,3)',
            'call fill(u,a)\nprint *,a%x\nprint *,a%y').replace('close(u)\nend program',"""close(u)
contains
subroutine fill(unit,values)
integer,intent(in) :: unit
type(point),intent(out) :: values(:,:)
read(unit,*) values(:,3:1:-1)
end subroutine
end program""")
        self.check(source,'9 11 5 7 1 3\n10 12 6 8 2 4\n')

    def test_polymorphic_array_diagnosed(self):
        source=self.source('type(point) :: a(3)',
            'call fill(u,a)').replace('close(u)\nend program',"""close(u)
contains
subroutine fill(unit,values)
integer,intent(in) :: unit
class(point),intent(out) :: values(:)
read(unit,*) values(1:2)
end subroutine
end program""")
        self.reject(source,'polymorphic defined input dispatch')

    def test_vector_section(self):
        self.check(self.source('type(point) :: a(3)',
            'a%x=-1\na%y=-1\nread(u,*) a([3,1])\nprint *,a%x\nprint *,a%y'),'3 -1 1\n4 -1 2\n')

    def test_allocated_matrix(self):
        self.check(self.source('type(point),allocatable :: a(:,:)',
            'allocate(a(2,3))\nread(u,*) a\nprint *,a%x\nprint *,a%y'),'1 3 5 7 9 11\n2 4 6 8 10 12\n')

    def test_eof_stops_array(self):
        # Local gfortran also loses the negative parent status for array EOF.
        self.check(self.source('type(point) :: a(3)',
            'a%x=-1\na%y=-1\nread(u,*,iostat=status,iomsg=message) a\nprint *,status<0,a(1)%x,a(1)%y,a(3)%x,a(3)%y,len_trim(message)>0','1 2 3'),
            'T 1 2 -1 -1 T\n',reference=False)

    def test_scalar_index_evaluated_once(self):
        source=self.source('type(point) :: a(3)\ninteger :: calls',
            'calls=0\nread(u,*) a(index())\nprint *,calls,a(2)%x,a(2)%y').replace('close(u)\nend program',"""close(u)
contains
integer function index()
calls=calls+1
index=2
end function
end program""")
        self.check(source,'1 1 2\n')

    def test_formatted_element(self):
        self.check(self.source('type(point) :: a(3)',
            'read(u,"(dt)") a(2)\nprint *,a(2)%x,a(2)%y'),'1 2\n')

    def test_multidimensional_error_stops_outer_loops(self):
        source=self.source('type(point) :: a(2,3)',
            'a%x=-1\na%y=-1\nread(u,*,iostat=status) a\nprint *,status>0,a(1,1)%x,a(1,2)%x,a(2,3)%x','1 2 bad 4 5 6')
        self.check(source,'T 1 -1 -1\n',reference=False)

    def test_stride_evaluated_once(self):
        source=self.source('type(point) :: a(3)\ninteger :: calls',
            'calls=0\na%x=-1\nread(u,*) a(1:3:step())\nprint *,calls,a%x').replace('close(u)\nend program',"""close(u)
contains
integer function step()
calls=calls+1
step=2
end function
end program""")
        self.check(source,'1 1 -1 3\n')

    def test_explicit_array_format(self):
        # Separate child list READs expose the documented local gfortran discrepancy.
        self.check(self.source('type(point) :: a(3)','read(u,"(3dt)") a\nprint *,a%x\nprint *,a%y'),
            '1 3 5\n2 4 6\n',reference=False)

if __name__=='__main__':
    unittest.main()

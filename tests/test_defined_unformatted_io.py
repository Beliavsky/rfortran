"""Defined unformatted stream I/O: interpreter, native O0/O2, and gfortran."""
from pathlib import Path
import unittest
import test_deallocate

SOURCE = (Path(__file__).parent / 'programs/xdefined_unformatted_io.f90').read_text()

class DefinedUnformatted(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check(SOURCE, '0 unchanged\n113 7\n0 7 13 unchanged\n')

    def program(self, declarations, body):
        return SOURCE.split('program main')[0] + f"""program main
use binary_points
implicit none
integer :: unit,ios
character(80) :: msg
{declarations}
open(newunit=unit,file='defined-unformatted.bin',access='stream', &
     form='unformatted',status='replace')
msg='unchanged'
{body}
close(unit,status='delete')
end program
"""

    def test_arrays_and_mixed_list(self):
        self.check(self.program('type(point) :: p(2),q(2)\ninteger :: a,b', """
p(1)=point(1,2)
p(2)=point(3,4)
write(unit) 8,p,9
rewind(unit)
read(unit) a,q,b
print *,a,q%x,q%y,b
"""), '8 1 3 2 4 9\n')

    def test_triplet_sections(self):
        self.check(self.program('type(point) :: p(3),q(3)', """
p(1)=point(1,2)
p(2)=point(3,4)
p(3)=point(5,6)
write(unit) p(3:1:-2)
rewind(unit)
read(unit) q(1:3:2)
print *,q%x,q%y
"""), '5 0 1 6 0 2\n')

    def test_vector_sections(self):
        self.check(self.program('type(point) :: p(3),q(3)', """
p(1)=point(1,2)
p(2)=point(3,4)
p(3)=point(5,6)
write(unit) p([3,1])
rewind(unit)
read(unit) q([1,3])
print *,q%x,q%y
"""), '5 0 1 6 0 2\n', reference=False)  # gfortran warns about temporaries.

    def test_matrix(self):
        self.check(self.program('type(point) :: p(2,2),q(2,2)', """
p%x=reshape([1,2,3,4],[2,2])
p%y=reshape([5,6,7,8],[2,2])
write(unit) p(:,2:1:-1)
rewind(unit)
read(unit) q
print *,q%x,q%y
"""), '3 4 1 2 7 8 5 6\n')

    def test_implied_do(self):
        self.check(self.program('type(point) :: p(2),q(2)\ninteger :: i', """
p(1)=point(1,2)
p(2)=point(3,4)
write(unit) (p(i),i=2,1,-1)
rewind(unit)
read(unit) (q(i),i=1,2)
print *,q%x,q%y
"""), '3 1 4 2\n')

    def test_runtime_array(self):
        self.check(self.program('type(point),allocatable :: p(:),q(:)\ninteger :: n', """
n=2
allocate(p(n),q(n))
p(1)=point(1,2)
p(2)=point(3,4)
write(unit) p
rewind(unit)
read(unit) q
print *,q%x,q%y
"""), '1 3 2 4\n')

    def test_position(self):
        self.check(self.program('type(point) :: p,q', """
p=point(7,13)
write(unit) 99
write(unit,pos=5) p
read(unit,pos=5) q
print *,q%x,q%y
"""), '7 13\n')

    def test_unit_evaluated_once(self):
        source=SOURCE.replace('implicit none\ntype :: point', 'implicit none\ninteger :: calls=0\ntype :: point',1)
        source=source.replace('end module', '''integer function choose(u)
integer,intent(in) :: u
calls=calls+1
choose=u
end function
end module''')
        source=source.replace('write(unit,iostat=ios,iomsg=msg) p','write(choose(unit),iostat=ios,iomsg=msg) p')
        source=source.replace('close(unit,status=', 'print *,calls\nclose(unit,status=')
        self.check(source,'0 unchanged\n113 7\n0 7 13 unchanged\n1\n')

    def test_without_iostat_stops(self):
        self.check(self.program('type(point) :: q', 'read(unit) q'),
            error='unformatted transfer failed', reference=False)

    def test_intrinsic_child_write_error(self):
        source=self.program('type(point) :: p', '''
close(unit)
open(newunit=unit,file='defined-unformatted.bin',access='stream', &
     form='unformatted',status='old',action='read')
write(unit,iostat=ios,iomsg=msg) p
print *,ios>0,msg/='unchanged'
''')
        self.check(source,'T T\n')

    def test_eof_message(self):
        self.check(self.program('type(point) :: q', """
read(unit,iostat=ios,iomsg=msg) q
print *,ios<0,msg/='unchanged'
"""), 'T T\n')

    def test_user_error_stops_later_items(self):
        source=self.program('type(point) :: p(2)\ninteger :: n', """
p(1)=point(1,2)
p(2)=point(3,4)
write(unit,iostat=ios,iomsg=msg) p,99
print *,ios,trim(msg)
rewind(unit)
read(unit,iostat=ios) n
print *,ios<0
""")
        source=source.replace('write(unit,iostat=iostat,iomsg=iomsg) dtv%y+100,dtv%x',
            "iostat=123\niomsg='writer refused'")
        # gfortran continues to transfer the trailing intrinsic item after the
        # user writer returns an error; rfort stops the parent transfer.
        self.check(source,'123 writer refused\nT\n', reference=False)

    def test_read_user_error(self):
        source=self.program('type(point) :: q(2)\ninteger :: tail', """
tail=-1
read(unit,iostat=ios,iomsg=msg) q,tail
print *,ios,trim(msg),q%x,tail
""")
        source=source.replace('read(unit,iostat=iostat,iomsg=iomsg) encoded,dtv%x\nif (iostat==0) dtv%y=encoded-100',
            "dtv%x=9\niostat=123\niomsg='reader refused'")
        self.check(source,'123 reader refused 9 0 -1\n', reference=False)

    def test_nested_defined_transfers(self):
        source=self.program('type(box) :: p,q', """
p%value=point(7,13)
write(unit) p
rewind(unit)
read(unit) q
print *,q%value%x,q%value%y
""")
        source=source.replace('end type\ncontains', '''end type
type :: box
type(point) :: value
contains
procedure :: write_box
procedure :: read_box
generic :: write(unformatted) => write_box
generic :: read(unformatted) => read_box
end type
contains''',1)
        source=source.replace('end module', '''subroutine write_box(dtv,unit,iostat,iomsg)
class(box),intent(in) :: dtv
integer,intent(in) :: unit
integer,intent(out) :: iostat
character(*),intent(inout) :: iomsg
write(unit,iostat=iostat,iomsg=iomsg) dtv%value
end subroutine
subroutine read_box(dtv,unit,iostat,iomsg)
class(box),intent(inout) :: dtv
integer,intent(in) :: unit
integer,intent(out) :: iostat
character(*),intent(inout) :: iomsg
read(unit,iostat=iostat,iomsg=iomsg) dtv%value
end subroutine
end module''')
        self.check(source,'7 13\n')

    def test_inherited_bindings(self):
        source=SOURCE.replace('end type\ncontains',
            'end type\ntype,extends(point) :: child\nend type\ncontains',1)
        source=source.replace('type(point) :: p, q','type(child) :: p, q').replace('p=point(7,13)','p%x=7\np%y=13')
        self.check(source,'0 unchanged\n113 7\n0 7 13 unchanged\n')

    def test_polymorphic_allocatable_overrides(self):
        source=SOURCE.replace('end type\ncontains', '''end type
type,extends(point) :: child
contains
procedure :: write_binary => write_child
procedure :: read_binary => read_child
end type
contains''',1)
        module=SOURCE.split('contains\nsubroutine write_binary',1)[1].split('end module')[0]
        child=('subroutine write_binary'+module).replace('write_binary','write_child').replace('read_binary','read_child').replace('class(point)','class(child)').replace('+100','+200').replace('-100','-200')
        source=source.replace('end module',child+'end module')
        source=source.replace('type(point) :: p, q','class(point),allocatable :: p, q').replace('p=point(7,13)', 'allocate(child :: p,q)\np%x=7\np%y=13')
        self.check(source,'0 unchanged\n213 7\n0 7 13 unchanged\n')

    def test_polymorphic_allocatable_arrays(self):
        source=self.program('class(point),allocatable :: p(:),q(:)', """
allocate(p(2),q(2))
p(1)%x=1
p(1)%y=2
p(2)%x=3
p(2)%y=4
write(unit,iostat=ios,iomsg=msg) p
print *,ios,trim(msg)
rewind(unit)
read(unit,iostat=ios,iomsg=msg) q
print *,ios,q%x,q%y,trim(msg)
""")
        self.check(source,'0 unchanged\n0 1 3 2 4 unchanged\n')

    def test_formatted_and_unformatted_bindings(self):
        source=SOURCE.replace('generic :: write(unformatted) => write_binary',
            'generic :: write(unformatted) => write_binary\nprocedure :: write_text\ngeneric :: write(formatted) => write_text')
        source=source.replace('end module','''subroutine write_text(dtv,unit,iotype,v_list,iostat,iomsg)
class(point),intent(in) :: dtv
integer,intent(in) :: unit,v_list(:)
character(*),intent(in) :: iotype
integer,intent(out) :: iostat
character(*),intent(inout) :: iomsg
write(unit,'(2i3)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y
end subroutine
end module''').replace('p=point(7,13)', "p=point(7,13)\nprint '(dt)',p")
        self.check(source,'7 13\n0 unchanged\n113 7\n0 7 13 unchanged\n')

    def test_class_array_dummy(self):
        source=self.program('type(point) :: p(2),q(2)', """
p(1)=point(1,2)
p(2)=point(3,4)
call save(unit,p)
rewind(unit)
call load(unit,q)
print *,q%x,q%y
""")
        source=source.replace('end module','''subroutine save(u,p)
integer,intent(in) :: u
class(point),intent(in) :: p(:)
write(u) p
end subroutine
subroutine load(u,p)
integer,intent(in) :: u
class(point),intent(inout) :: p(:)
read(u) p
end subroutine
end module''')
        self.check(source,'1 3 2 4\n')

    def test_four_argument_interface(self):
        self.reject(SOURCE.replace('subroutine write_binary(dtv,unit,iostat,iomsg)',
            'subroutine write_binary(dtv,unit,iostat,iomsg,extra)\ninteger :: extra'),
            'requires four dummy arguments')

    def test_read_intent(self):
        self.reject(SOURCE.replace('class(point),intent(inout) :: dtv',
            'class(point),intent(out) :: dtv'), 'requires intent(inout)')

    def test_read_expression_rejected(self):
        self.reject(SOURCE.replace('read(unit,iostat=ios,iomsg=msg) q',
            'read(unit,iostat=ios,iomsg=msg) point(1,2)'), 'requires writable variables')

    def test_private_implementation_use_only(self):
        source=SOURCE.replace('implicit none\ntype :: point',
            'implicit none\nprivate\npublic :: point\ntype :: point',1)
        source=source.replace('use binary_points\n', 'use binary_points,only:point\n')
        self.check(source,'0 unchanged\n113 7\n0 7 13 unchanged\n')

    def test_write_constructor(self):
        source=SOURCE.replace('write(unit,iostat=ios,iomsg=msg) p',
            'write(unit,iostat=ios,iomsg=msg) point(7,13)')
        self.check(source,'0 unchanged\n113 7\n0 7 13 unchanged\n')

    def test_default_container_cannot_bypass_component_binding(self):
        for direction in ('read', 'write'):
            for dims in ('', '(2)'):
                with self.subTest(direction=direction, dims=dims):
                    source=self.program(f'type(holder) :: q{dims}', f'{direction}(unit) q')
                    source=source.replace('end type\ncontains',
                        'end type\ntype :: holder\ntype(point) :: value\nend type\ncontains',1)
                    self.reject(source,'with defined-I/O components is not yet supported')

if __name__ == '__main__':
    unittest.main()

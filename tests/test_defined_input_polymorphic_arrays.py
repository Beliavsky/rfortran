"""Defined READ of CLASS arrays and sections with concrete dynamic views."""
from pathlib import Path
import unittest
import test_deallocate

SOURCE=(Path(__file__).parent/'programs/xdefined_input_polymorphic_arrays.f90').read_text()

class PolymorphicArrays(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check(SOURCE,'3 1 4 2\n')

    def test_whole_array(self):
        self.check(SOURCE.replace('values(2:1:-1)','values'),'1 3 2 4\n')

    def test_vector_section(self):
        # gfortran -fcheck=all emits an array-temporary warning on this case.
        self.check(SOURCE.replace('values(2:1:-1)','values([2,1])'),'3 1 4 2\n',reference=False)

    def test_output_section_uses_dynamic_writer(self):
        source=SOURCE.replace('generic :: read(formatted) => read_value',
            'generic :: read(formatted) => read_value\nprocedure :: write_value => write_base\ngeneric :: write(formatted) => write_value')
        source=source.replace('procedure :: read_value => read_child',
            'procedure :: read_value => read_child\nprocedure :: write_value => write_child')
        writers=''
        for kind, fields in [('base','dtv%x'),('child','dtv%x,dtv%y')]:
            writers+=f"""subroutine write_{kind}(dtv,unit,iotype,v_list,iostat,iomsg)
class({kind}),intent(in) :: dtv
integer,intent(in) :: unit,v_list(:)
character(*),intent(in) :: iotype
integer,intent(out) :: iostat
character(*),intent(inout) :: iomsg
write(unit,'(2i3)',iostat=iostat,iomsg=iomsg) {fields}
end subroutine
"""
        source=source.replace('end module',writers+'end module').replace(
            "read(text,'(2dt)') values(2:1:-1)",
            "read(text,'(2dt)') values(2:1:-1)\nwrite(*,'(2dt)') values(2:1:-1)")
        self.check(source,'1 2 3 4\n3 1 4 2\n')

    def test_matrix_section(self):
        source=SOURCE.replace('values(:)','values(:,:)').replace('values(2)','values(2,2)').replace('values(2:1:-1)','values(:,2:1:-1)').replace("'(2dt)'","'(4dt)'").replace("'  1  2  3  4'","'  1  2  3  4  5  6  7  8'")
        self.check(source,'5 7 1 3 6 8 2 4\n')

    def test_nonunit_actual_bounds(self):
        source=SOURCE.replace('values(2)','values(-2:-1)')
        self.check(source,'3 1 4 2\n')

    def test_empty_section(self):
        self.check(SOURCE.replace('values(2:1:-1)','values(2:1)'),'0 0 0 0\n')

    def test_inherited_reader(self):
        source=SOURCE.replace('type(child) :: values','type(inherited) :: values').replace("'  1  2  3  4'","'  1  2'").replace('print *,values%x,values%y','print *,values%x')
        self.check(source,'2 1\n')

    def test_runtime_format_and_stride(self):
        source=SOURCE.replace("read(text,'(2dt)') values(2:1:-1)","character(20) :: fmt\ninteger :: step\nfmt='(2dt)'\nstep=-1\nread(text,fmt) values(2:1:step)")
        self.check(source,'3 1 4 2\n')

    def test_forwarding_class_dummy(self):
        source=SOURCE.replace('end module',"subroutine forward(values,text)\nclass(base),intent(inout) :: values(:)\ncharacter(*),intent(in) :: text\ncall fill_array(values,text)\nend subroutine\nend module").replace("call fill_array(values,'", "call forward(values,'")
        self.check(source,'3 1 4 2\n')

    def test_external_status(self):
        source=SOURCE.replace("read(text,'(2dt)') values(2:1:-1)","integer :: u,ios\ncharacter(80) :: msg\nopen(newunit=u,status='scratch')\nwrite(u,'(a)') text\nrewind(u)\nmsg='unchanged'\nread(u,'(2dt)',iostat=ios,iomsg=msg) values(2:1:-1)\nclose(u)\nprint *,ios,trim(msg)")
        self.check(source,'0 unchanged\n3 1 4 2\n')

    def test_rank_three(self):
        source=SOURCE.replace('values(:)','values(:,:,:)').replace('values(2)','values(1,1,2)').replace('values(2:1:-1)','values(1,1,2:1:-1)')
        self.check(source,'3 1 4 2\n')

    def test_rank_fifteen(self):
        source=SOURCE.replace('values(:)','values('+','.join([':']*15)+')').replace('values(2)','values('+','.join(['1']*14+['2'])+')').replace('values(2:1:-1)','values')
        self.check(source,'1 3 2 4\n')

    def test_runtime_sized_concrete_actual(self):
        source=SOURCE.replace('type(child) :: values(2)','type(child),allocatable :: values(:)\ninteger :: n').replace("call fill_array(values,'", "n=2\nallocate(values(n))\ncall fill_array(values,'")
        self.check(source,'3 1 4 2\n')

    def test_error_stops_remaining_elements(self):
        source=SOURCE.replace("read(text,'(2dt)') values(2:1:-1)","integer :: ios\ncharacter(80) :: msg\nread(text,'(2dt)',iostat=ios,iomsg=msg) values\nprint *,ios/=0,len_trim(msg)>0").replace("'  1  2  3  4'","'  1bad  3  4'").replace('print *,values%x,values%y','print *,values(2)%x,values(2)%y')
        self.check(source,'T T\n0 0\n')

    def test_intent_out(self):
        source=SOURCE.replace('class(base),intent(inout) :: values','class(base),intent(out) :: values')
        self.check(source,'3 1 4 2\n')

    def test_section_actual(self):
        source=SOURCE.replace("call fill_array(values,'", "call fill_array(values(2:1:-1),'")
        self.check(source,'1 3 2 4\n')

    def test_section_actual_function_diagnosed(self):
        source=SOURCE.replace("call fill_array(values,'", "call fill_array(values(last():1:-1),'").replace('end program', 'contains\ninteger function last()\nlast=2\nend function\nend program')
        self.reject(source,'CLASS array section actuals with function calls')

    def test_declared_type_generic_resolution(self):
        source=SOURCE.replace('contains\nsubroutine read_base', 'interface mark\nmodule procedure mark_base,mark_child\nend interface\ncontains\nsubroutine read_base',1)
        source=source.replace('end module', "subroutine mark_base(x)\ntype(base),intent(in) :: x(:)\nprint *, 'base',size(x)\nend subroutine\nsubroutine mark_child(x)\ntype(child),intent(in) :: x(:)\nprint *, 'child',size(x)\nend subroutine\nend module")
        source=source.replace("read(text,'(2dt)') values(2:1:-1)", "read(text,'(2dt)') values(2:1:-1)\ncall mark(values)")
        self.check(source,'base 2\n3 1 4 2\n',reference=False)  # gfortran -fcheck=all warns about a temporary.

    def test_polymorphic_allocatable_direct(self):
        source=SOURCE.split('program main')[0]+"""program main
use polymorphic_input
implicit none
class(base),allocatable :: values(:)
character(12) :: text
allocate(child :: values(2))
text='  1  2  3  4'
read(text,'(2dt)') values(2:1:-1)
select type(values)
type is(child)
 print *,values%x,values%y
end select
deallocate(values)
end program
"""
        self.check(source,'3 1 4 2\n')

    def test_polymorphic_allocatable_through_dummy(self):
        source=SOURCE.split('program main')[0]+"""program main
use polymorphic_input
implicit none
class(base),allocatable :: values(:)
allocate(child :: values(2))
call fill_array(values,'  1  2  3  4')
select type(values)
type is(child)
 print *,values%x,values%y
end select
deallocate(values)
end program
"""
        self.check(source,'3 1 4 2\n')

    def test_polymorphic_allocatable_error(self):
        source=SOURCE.split('program main')[0]+"""program main
use polymorphic_input
implicit none
class(base),allocatable :: values(:)
integer :: ios
character(80) :: msg
character(12) :: text
allocate(child :: values(2))
text='  1bad  3  4'
msg='unchanged'
read(text,'(2dt)',iostat=ios,iomsg=msg) values
print *,ios/=0,msg/='unchanged'
select type(values)
type is(child)
 print *,values(2)%x,values(2)%y
end select
deallocate(values)
end program
"""
        self.check(source,'T T\n0 0\n')

if __name__=='__main__':
    unittest.main()

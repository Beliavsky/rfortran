"""Defined formatted I/O with scalar CHARACTER internal files."""
from pathlib import Path
import unittest
import test_deallocate
import test_defined_io as output
import test_defined_input_formats as inputs

class InternalDefinedIO(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((Path(__file__).parent/'programs/xdefined_formatted_internal.f90').read_text(),
            '10 1 2 3 4 5.5\n10 1 2 3 4 5.5\n')

    def source(self,main,child="write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x",declarations=''):
        return output.DefinedIO().source(child,main).replace('type(point) :: a,b',
            'type(point) :: a,b\ncharacter(40) :: buffer\ninteger :: status\ncharacter(100) :: message\n'+declarations)

    def test_literal_write(self):
        self.check(self.source("write(buffer,'(dt,1x,dt)') a,b\nprint '(a)',trim(buffer)"),'7 9\n')

    def test_list_write(self):
        self.check(self.source("write(buffer,*) a,b\nprint '(a)',trim(buffer)"),'7 9\n')

    def test_overflow(self):
        source=self.source("write(buffer,'(dt,i0)',iostat=status,iomsg=message) a,999\nprint *,status<0,len_trim(message)>0")
        source=source.replace('character(40) :: buffer','character(1) :: buffer')
        self.check(source,'T T\n')

    def test_error_message(self):
        self.check(self.source("write(buffer,'(dt)',iostat=status,iomsg=message) a\nprint *,status,trim(message)",
            "iostat=17\niomsg='writer failed'"),'17 writer failed\n')

    def input_source(self,declarations,body):
        module=(Path(__file__).parent/'programs/xdefined_formatted_internal.f90').read_text().split('program main')[0]
        return module+'program main\nuse runtime_defined_io\nimplicit none\n'+declarations+'\n'+body+'\nend program\n'

    def test_input_character_variable(self):
        self.check(self.input_source('type(point) :: p\ncharacter(6) :: buffer',"buffer='  7  9'\nread(buffer,'(dt)') p\nprint *,p%x,p%y"),'7 9\n')

    def test_input_list_directed(self):
        source=self.input_source('type(point) :: p\ncharacter(3) :: buffer',"buffer='7 9'\nread(buffer,*) p\nprint *,p%x,p%y")
        source=source.replace("read(unit,'(2i3)',iostat=iostat,iomsg=iomsg)","read(unit,*,iostat=iostat,iomsg=iomsg)")
        self.check(source,'7 9\n')

    def test_input_error_stops_later_item(self):
        self.check(self.input_source('type(point) :: p\ninteger :: n,status\ncharacter(100) :: message\ncharacter(9) :: buffer',
            "n=-1\nbuffer='bad  9 11'\nread(buffer,'(dt,i3)',iostat=status,iomsg=message) p,n\nprint *,status>0,n,len_trim(message)>0"),
            'T -1 T\n',reference=False)

    def test_input_reversion_exhausts_scalar_file(self):
        self.check(self.input_source('type(point) :: p(2)\ninteger :: status\ncharacter(100) :: message\ncharacter(6) :: buffer',
            "p%x=-1\nbuffer='  7  9'\nread(buffer,'(dt)',iostat=status,iomsg=message) p\nprint *,status<0,p(1)%x,p(2)%x,len_trim(message)>0"),
            'T 7 -1 T\n',reference=False)

    def test_output_slash(self):
        self.check(self.source("write(buffer,'(dt,/,dt)',iostat=status,iomsg=message) a,b\nprint *,status<0,len_trim(message)>0"),'T T\n')

    def test_output_array_element(self):
        self.check(self.source("write(records(2),'(dt,1x,dt)') a,b\nprint '(a)',trim(records(2))",
            declarations='character(40) :: records(3)'), '7 9\n')

    def test_input_array_element(self):
        self.check(self.input_source('type(point) :: p\ncharacter(20) :: records(2)',
            "records(2)='  7  9'\nread(records(2),'(dt)') p\nprint *,p%x,p%y"),'7 9\n')

    def test_assumed_length_buffer(self):
        source=self.source("call show(buffer,a)\nprint '(a)',trim(buffer)")
        source=source.replace('end module',"""subroutine show(text,p)
character(*),intent(out) :: text
type(point),intent(in) :: p
write(text,'(dt)') p
end subroutine
end module""")
        self.check(source,'7\n')

    def test_cleanup_after_failure(self):
        self.check(self.source("do i=1,1100\nwrite(buffer,'(dt)',iostat=status,iomsg=message) a\nif(status/=17) stop 1\nend do\nprint *,status",
            "iostat=17\niomsg='failed'",'integer :: i'),'17\n')

    def test_success_preserves_parent_iomsg(self):
        self.check(self.source("message='unchanged'\nwrite(buffer,'(dt)',iostat=status,iomsg=message) a\nprint *,status,trim(message),trim(buffer)"),
            '0 unchanged7\n')

    def test_error_without_iostat(self):
        source=self.source("write(buffer,'(dt)') a", "iostat=17\niomsg='writer failed'")
        self.check(source,error='writer failed',reference=False)

    def test_literal_unit_is_rejected(self):
        self.reject(self.input_source('type(point) :: p',"read('  7  9','(dt)') p"),
            'defined internal I/O requires a CHARACTER variable')

    def test_child_opens_another_unit(self):
        child="""integer :: u,q
open(newunit=u,status='scratch')
write(u,'(i0)') 123
rewind(u)
read(u,*) q
close(u)
write(unit,'(i0)',iostat=iostat,iomsg=iomsg) dtv%x+q"""
        self.check(self.source("write(buffer,'(dt)') a\nprint '(a)',trim(buffer)",child),'130\n')

    def test_nested_internal_files(self):
        source=self.input_source('type(holder) :: a,b\ncharacter(30) :: buffer',
            "a%p%x=7\na%p%y=9\nwrite(buffer,'(dt)') a\nread(buffer,'(dt)') b\nprint *,b%p%x,b%p%y")
        source=source.replace('end type','end type\ntype :: holder\n   type(point) :: p\ncontains\n   procedure :: read_holder,write_holder\n   generic :: read(formatted) => read_holder\n   generic :: write(formatted) => write_holder\nend type',1)
        source=source.replace('end module',"subroutine read_holder(dtv,unit,iotype,v_list,iostat,iomsg)\nclass(holder),intent(inout) :: dtv\ninteger,intent(in) :: unit,v_list(:)\ncharacter(*),intent(in) :: iotype\ninteger,intent(out) :: iostat\ncharacter(*),intent(inout) :: iomsg\ncharacter(6) :: local\ntype(point) :: temporary\nlocal=' 20 30'\nread(local,'(dt)',iostat=iostat,iomsg=iomsg) temporary\nif(iostat/=0) return\nread(unit,'(dt)',iostat=iostat,iomsg=iomsg) dtv%p\nif(iostat==0) dtv%p%x=dtv%p%x+temporary%x\nend subroutine\nsubroutine write_holder(dtv,unit,iotype,v_list,iostat,iomsg)\nclass(holder),intent(in) :: dtv\ninteger,intent(in) :: unit,v_list(:)\ncharacter(*),intent(in) :: iotype\ninteger,intent(out) :: iostat\ncharacter(*),intent(inout) :: iomsg\ncharacter(6) :: local\nwrite(local,'(dt)',iostat=iostat,iomsg=iomsg) dtv%p\nif(iostat/=0) return\nwrite(unit,'(a6)',iostat=iostat,iomsg=iomsg) local\nend subroutine\nend module")
        self.check(source,'27 9\n')

    def test_output_subscript_evaluated_once(self):
        source=self.source("write(records(which()),'(dt)') a\nprint *,calls,trim(records(2))",declarations='character(40) :: records(3)')
        source=source.replace('implicit none','implicit none\ninteger :: calls=0',1)
        source=source.replace('end module',"""integer function which()
calls=calls+1
which=2
end function
end module""")
        self.check(source,'1 7\n')

    def test_character_array_file_boundary(self):
        self.reject(self.source("write(records,'(dt)') a",declarations='character(40) :: records(3)'),
            'defined internal I/O currently requires a scalar CHARACTER file')

if __name__=='__main__':
    unittest.main()

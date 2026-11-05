"""Shared T/TL record positioning across defined parent and child transfers."""
import unittest
from pathlib import Path
import test_deallocate
import test_defined_io as output
import test_defined_input as inputs
import test_defined_io_internal as internal

class Positioning(unittest.TestCase):
    check=test_deallocate.Deallocate.check

    def output(self,child,main):
        return output.DefinedIO().source(child,main)

    def test_child_output_absolute(self):
        self.check(self.output("write(unit,'(a,t1,i1)',iostat=iostat,iomsg=iomsg) 'abc',dtv%x", "write(*,'(dt)') a"),'7bc\n')

    def test_parent_output_absolute(self):
        self.check(self.output("write(unit,'(i1)',iostat=iostat,iomsg=iomsg) dtv%x", "write(*,'(a,dt,t2,a)') 'abc',a,'X'"),'aXc7\n')

    def test_child_output_left(self):
        self.check(self.output("write(unit,'(a,tl2,i1)',iostat=iostat,iomsg=iomsg) 'abcd',dtv%x", "write(*,'(dt)') a"),'ab7d\n')

    def test_output_runtime_formats(self):
        source=self.output("character(20) :: fmt\nfmt='(a,tl2,i1)'\nwrite(unit,fmt,iostat=iostat,iomsg=iomsg) 'abcd',dtv%x", "fmt='(dt,t2,a)'\nwrite(*,fmt) a,'X'")
        source=source.replace('type(point) :: a,b','type(point) :: a,b\ncharacter(20) :: fmt')
        self.check(source,'aX7d\n')

    def test_child_input_absolute(self):
        source=inputs.DefinedInput().fixed().replace('read(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%y','read(unit,"(t1,i3)",iostat=iostat,iomsg=iomsg) dtv%y')
        self.check(inputs.DefinedInput().main(source,'read(u,"(dt)") a\nprint *,a%x,a%y'),'7 7\n')

    def test_parent_input_absolute(self):
        source=inputs.DefinedInput().fixed().replace('integer :: u,status','integer :: u,status,n')
        self.check(inputs.DefinedInput().main(source,'read(u,"(dt,t4,i3)") a,n\nprint *,a%x,a%y,n'),'7 9 9\n')

    def test_child_input_left(self):
        source=inputs.DefinedInput().fixed().replace('read(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%x','read(unit,"(i5)",iostat=iostat,iomsg=iomsg) dtv%x').replace('read(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%y','read(unit,"(tl5,i2)",iostat=iostat,iomsg=iomsg) dtv%y').replace("'  7  9 99'","' 7   '")
        self.check(inputs.DefinedInput().main(source,'read(u,"(dt)") a\nprint *,a%x,a%y'),'7 7\n')

    def test_input_internal_records(self):
        source=internal.InternalDefinedIO().input_source('type(point) :: p(2)\ncharacter(6) :: buffer(2)', "buffer=['  1  2','  3  4']\nread(buffer,'(dt)') p\nprint *,p(1)%x,p(1)%y,p(2)%x,p(2)%y")
        source=source.replace("read(unit,'(2i3)',", "read(unit,'(i3,t1,i3)',")
        self.check(source,'1 1 3 3\n')

    def test_output_internal_records(self):
        self.check(internal.InternalDefinedIO().source("write(records,'(dt,/,dt)') a,b\nprint '(a)',records", "write(unit,'(a,t2,i1,a)',iostat=iostat,iomsg=iomsg) 'abc',dtv%x,'c'",'character(3) :: records(2)'), 'a7c\na9c\n')

    def test_fixture(self):
        self.check((Path(__file__).parent/'programs/xdefined_io_positioning.f90').read_text(),'[7]\n[9]\n7 9 7 9\n')

    def test_long_output_record(self):
        source=internal.InternalDefinedIO().source("write(buffer,'(dt)') a\nprint *,buffer(1:3),len_trim(buffer)", "write(unit,'(a,t1,i1,t5000,a)',iostat=iostat,iomsg=iomsg) repeat('x',5000),dtv%x,'x'")
        source=source.replace('character(40) :: buffer','character(6000) :: buffer')
        self.check(source,'7xx 5000\n')

    def test_nested_output(self):
        source=internal.InternalDefinedIO().input_source('type(holder) :: h\ncharacter(4) :: buffer', "h%p%x=7\nh%p%y=0\nwrite(buffer,'(dt)') h\nprint '(a)',buffer")
        source=source.replace('contains\nsubroutine read_point', "type :: holder\n type(point) :: p\ncontains\n procedure :: write_holder\n generic :: write(formatted) => write_holder\nend type\ncontains\nsubroutine write_holder(dtv,unit,iotype,v_list,iostat,iomsg)\nclass(holder),intent(in) :: dtv\ninteger,intent(in) :: unit,v_list(:)\ncharacter(*),intent(in) :: iotype\ninteger,intent(out) :: iostat\ncharacter(*),intent(inout) :: iomsg\ncharacter(20) :: fmt\nfmt='(a,dt,t1,a,t4,a)'\nwrite(unit,fmt,iostat=iostat,iomsg=iomsg) 'abc',dtv%p,'Z','7'\nend subroutine\nsubroutine read_point")
        source=source.replace("write(unit,'(2i3)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y", "write(unit,'(t4,i1)',iostat=iostat,iomsg=iomsg) dtv%x")
        self.check(source,'Zbc7\n')

    def test_list_input_then_position(self):
        source=inputs.DefinedInput().source().replace('read(unit,*,iostat=iostat,iomsg=iomsg) dtv%y','read(unit,"(t1,i1)",iostat=iostat,iomsg=iomsg) dtv%y')
        # gfortran 17 reads 9 here despite T1; assert the shared-position result.
        self.check(inputs.DefinedInput().main(source,'read(u,"(dt)") a\nprint *,a%x,a%y'),'7 7\n',reference=False)

    def test_long_input_record(self):
        source=internal.InternalDefinedIO().input_source('type(point) :: p\ncharacter(5001) :: buffer', "buffer=repeat(' ',5000)//'7'\nread(buffer,'(dt)') p\nprint *,p%x,p%y")
        source=source.replace("read(unit,'(2i3)',", "read(unit,'(t5001,i1,tl1,i1)',")
        self.check(source,'7 7\n')

if __name__=='__main__':
    unittest.main()

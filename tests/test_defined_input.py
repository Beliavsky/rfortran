"""Defined formatted input in interpreter/native O0/O2 and gfortran."""
import unittest
import test_deallocate
from test_record_read import ROOT

class DefinedInput(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def source(self):
        return (ROOT/'tests/programs/xdefined_formatted_input.f90').read_text()

    def test_child_records(self):
        self.check(self.source(),'7 9 11 13 0 unchanged\n')

    def main(self, source, body):
        return source[:source.index('rewind(u)')]+'rewind(u)\n'+body+'\nclose(u)\nend program\n'

    def test_multiple_objects_share_record(self):
        source=self.source().replace("'7 9 99'","'1 2 3 4 99'")
        self.check(self.main(source,"read(u,*) a,b\nprint *,a%x,a%y,b%x,b%y"),'1 2 3 4\n')

    def test_end_of_file_status(self):
        self.check(self.main(self.source(),"""read(u,*) a
read(u,*) b
message='unchanged'
read(u,*,iostat=status,iomsg=message) a
print *,status<0,len_trim(message)>0"""),'T T\n')

    def test_bad_input_status(self):
        source=self.source().replace("'7 9 99'","'bad 9'")
        self.check(self.main(source,"read(u,*,iostat=status,iomsg=message) a\nprint *,status>0,len_trim(message)>0"),'T T\n')

    def test_user_error_message(self):
        source=self.source()
        start=source.index('read(unit,')
        end=source.index('end subroutine',start)
        source=source[:start]+"iostat=17\niomsg='user input failure'\n"+source[end:]
        self.check(self.main(source,"read(u,*,iostat=status,iomsg=message) a\nprint *,status,trim(message)"),
            '17 user input failure\n')

    def test_unhandled_error(self):
        source=self.source().replace("'7 9 99'","'bad 9'")
        self.check(self.main(source,"read(u,*) a"),error='list-directed input failed',reference=False)

    def test_dt_parameters(self):
        source=self.source().replace('read(unit,*,iostat=iostat,iomsg=iomsg) dtv%x',
            "print *,trim(iotype),size(v_list),v_list\nread(unit,*,iostat=iostat,iomsg=iomsg) dtv%x")
        self.check(self.main(source,"read(u,\"(dt'point'(3,2))\") a\nprint *,a%x,a%y"),'DTpoint 2 3 2\n7 9\n')

    def test_private_implementation_use_only(self):
        source=self.source().replace('implicit none','implicit none\nprivate\npublic :: point',1)
        source=source.replace('use defined_input','use defined_input, only: point')
        self.check(source,'7 9 11 13 0 unchanged\n')

    def test_invalid_dtv_intent(self):
        self.reject(self.source().replace('class(point),intent(inout)','class(point),intent(in)'),'requires intent(inout)')

    def fixed(self):
        return self.source().replace('read(unit,*,','read(unit,"(i3)",').replace("'7 9 99'","'  7  9 99'").replace("'11 13 99'","' 11 13 99'")

    def test_explicit_child_format(self):
        # The local gfortran build skips the second field for the list-directed parent.
        self.check(self.fixed(),'7 9 11 13 0 unchanged\n',reference=False)

    def test_explicit_parent_and_children(self):
        self.check((ROOT/'tests/programs/xdefined_formatted_input_fields.f90').read_text(),'7 9 11 13 0 unchanged\n')

    def test_explicit_child_bad_input(self):
        source=self.fixed().replace("'  7  9 99'","'bad  9'")
        self.check(self.main(source,'read(u,*,iostat=status,iomsg=message) a\nprint *,status>0,len_trim(message)>0'),'T T\n')

    def test_explicit_child_eof(self):
        self.check(self.main(self.fixed(),"""read(u,*) a
read(u,*) b
read(u,*,iostat=status,iomsg=message) a
print *,status<0,len_trim(message)>0"""),'T T\n')

    def test_explicit_then_list_child(self):
        source=self.fixed().replace('read(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%y','read(unit,*,iostat=iostat,iomsg=iomsg) dtv%y')
        self.check(source,'7 9 11 13 0 unchanged\n')

    def test_list_then_explicit_child(self):
        source=self.fixed().replace('read(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%x','read(unit,*,iostat=iostat,iomsg=iomsg) dtv%x')
        # Same local gfortran list-parent field-skipping discrepancy.
        self.check(source,'7 9 11 13 0 unchanged\n',reference=False)

    def test_child_left_tab_diagnosed(self):
        self.reject(self.fixed().replace('(i3)','(i3,t1)'),'T or TL positioning')

    def test_child_runtime_format_diagnosed(self):
        self.reject(self.source().replace('read(unit,*,','read(unit,iotype,'),'constant format')

    def test_explicit_child_trailing_spaces(self):
        source=self.fixed().replace('read(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%x','read(unit,"(i3,2x)",iostat=iostat,iomsg=iomsg) dtv%x')
        source=source.replace("'  7  9 99'","'  7xx  9 99'")
        self.check(self.main(source,'read(u,"(dt)") a\nprint *,a%x,a%y'),'7 9\n')

    def test_explicit_child_slash(self):
        source=self.fixed().replace('read(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%x','read(unit,"(i3,/)",iostat=iostat,iomsg=iomsg) dtv%x')
        self.check(self.main(source,'read(u,*) a\nprint *,a%x,a%y'),'7 11\n')

    def test_explicit_child_reversion(self):
        source=self.fixed()
        start=source.index('read(unit,')
        end=source.index('end subroutine',start)
        source=source[:start]+'read(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%x,dtv%y\n'+source[end:]
        self.check(self.main(source,'read(u,*) a\nprint *,a%x,a%y'),'7 11\n')

    def test_explicit_child_character_real_logical(self):
        source=self.fixed().replace('integer :: x,y','integer :: x,y\ncharacter(3) :: label\nreal :: weight\nlogical :: yes')
        source=source.replace('read(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%x',
            'read(unit,"(a3,1x,f5.2,1x,l1,1x,i3)",iostat=iostat,iomsg=iomsg) dtv%label,dtv%weight,dtv%yes,dtv%x')
        source=source.replace("'  7  9 99'","'abc 12.34 T   7  9 99'")
        self.check(self.main(source,'read(u,*) a\nprint *,a%label,abs(a%weight-12.34)<0.001,a%yes,a%x,a%y'),'abc T T 7 9\n')

    def test_explicit_child_plain_a(self):
        source=self.fixed().replace('integer :: x,y','integer :: x,y\ncharacter(3) :: label')
        source=source.replace('read(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%x',
            'read(unit,"(a)",iostat=iostat,iomsg=iomsg) dtv%label\nread(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%x')
        source=source.replace("'  7  9 99'","'abc  7  9 99'")
        self.check(self.main(source,'read(u,*) a\nprint *,a%label,a%x,a%y'),'abc 7 9\n')

    def test_explicit_child_plain_a_without_status(self):
        source=self.fixed().replace('integer :: x,y','integer :: x,y\ncharacter(3) :: label')
        source=source.replace('read(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%x',
            'read(unit,"(a)") dtv%label\nread(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%x')
        source=source.replace("'  7  9 99'","'abc  7  9 99'")
        self.check(self.main(source,'read(u,"(dt)") a\nprint *,a%label,a%x,a%y'),'abc 7 9\n')

    def test_explicit_reader_helper(self):
        source=self.fixed().replace('read(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%x',
            'call field(unit,dtv%x,iostat,iomsg)')
        source=source.replace('end module',"""subroutine field(unit,x,status,message)
integer,intent(in) :: unit
integer,intent(out) :: x,status
character(len=*),intent(inout) :: message
read(unit,'(i3)',iostat=status,iomsg=message) x
end subroutine
end module""")
        self.check(self.main(source,'read(u,"(dt)") a\nprint *,a%x,a%y'),'7 9\n')

    def test_explicit_other_unit_record_advancement(self):
        source=self.fixed().replace('read(unit,"(i3)",iostat=iostat,iomsg=iomsg) dtv%x',"""integer :: other
open(unit=21,status='scratch')
write(21,'(a)') ' 20 30'
write(21,'(a)') ' 40 50'
rewind(21)
read(21,'(i3)') other
print *,other
read(21,'(i3)') other
print *,other
close(21)
read(unit,'(i3)',iostat=iostat,iomsg=iomsg) dtv%x""")
        self.check(self.main(source,'read(u,"(dt)") a\nprint *,a%x,a%y'),'20\n40\n7 9\n')

    def test_internal_parent_diagnosed(self):
        source=self.source().replace('read(u,*) a',"read('7 9',*) a")
        self.reject(source,'external INTEGER unit')

    def test_other_unit_keeps_its_own_record(self):
        source=self.source().replace('read(unit,*,iostat=iostat,iomsg=iomsg) dtv%x',"""integer :: other
open(unit=21,status='scratch')
write(21,'(a)') '20 30'
write(21,'(a)') '40 50'
rewind(21)
read(21,*) other
print *,other
read(21,*) other
print *,other
close(21)
read(unit,*,iostat=iostat,iomsg=iomsg) dtv%x""")
        self.check(self.main(source,'read(u,*) a\nprint *,a%x,a%y'),'20\n40\n7 9\n')

    def test_nested_reader(self):
        source=self.source().replace('contains\nsubroutine read_point',"""type :: wrapper
type(point) :: inner
contains
procedure :: read_wrapper
generic :: read(formatted) => read_wrapper
end type
contains
subroutine read_wrapper(dtv,unit,iotype,v_list,iostat,iomsg)
class(wrapper),intent(inout) :: dtv
integer,intent(in) :: unit
character(len=*),intent(in) :: iotype
integer,intent(in) :: v_list(:)
integer,intent(out) :: iostat
character(len=*),intent(inout) :: iomsg
read(unit,*,iostat=iostat,iomsg=iomsg) dtv%inner
end subroutine
subroutine read_point""")
        source=source.replace('type(point) :: a,b','type(point) :: a,b\ntype(wrapper) :: w')
        self.check(self.main(source,'read(u,*) w\nprint *,w%inner%x,w%inner%y'),'7 9\n')

    def test_child_message_on_error(self):
        source=self.source().replace("'7 9 99'","'bad 9'")
        source=source.replace('if (iostat /= 0) return',"""if (iostat /= 0) then
print *,len_trim(iomsg)>0
return
end if""")
        self.check(self.main(source,'read(u,*,iostat=status,iomsg=message) a\nprint *,status>0'),'T\nT\n')

    def test_reader_helper(self):
        source=self.source().replace('read(unit,*,iostat=iostat,iomsg=iomsg) dtv%x',
            'call field(unit,dtv%x,iostat,iomsg)')
        source=source.replace('end module',"""subroutine field(unit,x,status,message)
integer,intent(in) :: unit
integer,intent(out) :: x,status
character(len=*),intent(inout) :: message
read(unit,*,iostat=status,iomsg=message) x
end subroutine
end module""")
        self.check(source,'7 9 11 13 0 unchanged\n')

    def test_inherited_reader(self):
        source=self.source().replace('contains\nsubroutine read_point',
            'type,extends(point) :: child\ninteger :: extra\nend type\ncontains\nsubroutine read_point')
        source=source.replace('type(point) :: a,b','type(child) :: a,b')
        self.check(source,'7 9 11 13 0 unchanged\n')

    def test_both_input_and_output_bindings(self):
        source=self.source().replace('procedure :: read_point','procedure :: read_point,write_point\n   generic :: write(formatted) => write_point')
        source=source.replace('end module',"""subroutine write_point(dtv,unit,iotype,v_list,iostat,iomsg)
class(point),intent(in) :: dtv
integer,intent(in) :: unit
character(len=*),intent(in) :: iotype
integer,intent(in) :: v_list(:)
integer,intent(out) :: iostat
character(len=*),intent(inout) :: iomsg
write(unit,'(i0,1x,i0)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y
end subroutine
end module""")
        self.check(self.main(source,'read(u,*) a\nwrite(*,"(dt)") a'),'7 9\n')

    def test_wrong_status_type(self):
        source=self.source().replace('integer :: u,status','integer :: u\nreal :: status')
        self.reject(source,'IOSTAT requires a scalar INTEGER')

    def test_wrong_message_type(self):
        source=self.source().replace('character(100) :: message','integer :: message')
        source=self.main(source,'read(u,*,iostat=status,iomsg=message) a')
        self.reject(source,'IOMSG requires a scalar CHARACTER')

    def test_multiple_dt_objects(self):
        source=self.source().replace("'7 9 99'","'1 2 3 4 99'")
        # The local gfortran 17 build skips the 3 and produces 1,2,4,99.
        # Keep an explicit value/position assertion for this case.
        self.check(self.main(source,'read(u,"(2dt)") a,b\nprint *,a%x,a%y,b%x,b%y'),'1 2 3 4\n',reference=False)

    def test_reader_error_stops_later_items(self):
        # The local gfortran 17 build invokes the reader twice despite IOSTAT=17.
        source=self.source().replace('implicit none','implicit none\ninteger :: calls=0',1)
        start=source.index('read(unit,')
        end=source.index('end subroutine',start)
        source=source[:start]+"calls=calls+1\niostat=17\niomsg='stop here'\n"+source[end:]
        self.check(self.main(source,'read(u,*,iostat=status,iomsg=message) a,b\nprint *,calls,status,trim(message)'),
            '1 17 stop here\n',reference=False)

    def test_ordinary_read_after_parent(self):
        source=self.source().replace("write(u,'(a)') '11 13 99'","write(u,'(a)') '11 13 99'\nwrite(u,'(a)') '17 19'")
        self.check(self.main(source,"read(u,*) a\nread(u,*) b\nread(u,*) status\nprint *,status"),'17\n')

if __name__=='__main__':
    unittest.main()

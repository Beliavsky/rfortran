"""Runtime formats in defined readers and their helpers."""
from pathlib import Path
import unittest
import test_deallocate
import test_defined_input as inputs

class DynamicChild(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def source(self, fmt='(i3)', transfer=None):
        source=inputs.DefinedInput().fixed()
        start=source.index('read(unit,')
        source=source[:start]+"character(40) :: fmt\nfmt='"+fmt+"'\n"+source[start:]
        if transfer is None:
            source=source.replace('read(unit,"(i3)",','read(unit,fmt,')
        else:
            start=source.index('read(unit,')
            end=source.index('end subroutine', start)
            source=source[:start]+transfer+'\n'+source[end:]
        return source

    def main(self, source, body):
        return inputs.DefinedInput().main(source,body)

    def test_record_continuation(self):
        self.check(self.source(), '7 9 11 13 0 unchanged\n',reference=False)

    def test_explicit_parent(self):
        self.check(self.main(self.source(), 'read(u,"(dt)") a\nprint *,a%x,a%y'), '7 9\n')

    def test_reversion(self):
        source=self.source(transfer='read(unit,fmt,iostat=iostat,iomsg=iomsg) dtv%x,dtv%y')
        self.check(self.main(source,'read(u,"(dt)") a\nprint *,a%x,a%y'),'7 11\n')

    def test_slash(self):
        self.check(self.main(self.source('(i3,/)'),'read(u,"(dt)",iostat=status) a\nprint *,a%x,a%y'),'7 11\n')

    def test_bad_field_stops_later_item(self):
        source=self.source('(2i3)', 'read(unit,fmt,iostat=iostat,iomsg=iomsg) dtv%x,dtv%y')
        source=source.replace("'  7  9 99'", "'bad  9 99'")
        self.check(self.main(source, 'a%y=-1\nread(u,"(dt)",iostat=status,iomsg=message) a\nprint *,status>0,a%y,len_trim(message)>0'), 'T -1 T\n',reference=False)

    def test_runtime_left_tab_error(self):
        self.check(self.main(self.source('(i3,t1)'), 'read(u,"(dt)",iostat=status,iomsg=message) a\nprint *,status>0,len_trim(message)>0'), 'T T\n',reference=False)

    def test_helper_assumed_length_format(self):
        source=self.source('(2i3)', 'call fields(unit,fmt,dtv%x,dtv%y,iostat,iomsg)')
        source=source.replace('end module', 'subroutine fields(unit,fmt,x,y,status,message)\ninteger,intent(in) :: unit\ncharacter(*),intent(in) :: fmt\ninteger,intent(out) :: x,y,status\ncharacter(*),intent(inout) :: message\nread(unit,fmt,iostat=status,iomsg=message) x,y\nend subroutine\nend module')
        self.check(self.main(source,'read(u,"(dt)") a\nprint *,a%x,a%y'),'7 9\n')

    def test_internal_array_fixture(self):
        self.check((Path(__file__).parent/'programs/xdefined_input_runtime_child.f90').read_text(),'1 2 3 4\n')

    def test_nested_readers(self):
        import test_defined_io_internal as internal
        # The inner point reader uses a runtime format while the holder reader
        # invokes it through its own runtime DT format.
        source=internal.InternalDefinedIO().input_source('type(holder) :: h\ncharacter(6) :: buffer', "buffer='  7  9'\nread(buffer,'(dt)') h\nprint *,h%p%x,h%p%y")
        source=source.replace('contains\nsubroutine read_point', "type :: holder\n type(point) :: p\ncontains\n procedure :: read_holder\n generic :: read(formatted) => read_holder\nend type\ncontains\nsubroutine read_holder(dtv,unit,iotype,v_list,iostat,iomsg)\nclass(holder),intent(inout) :: dtv\ninteger,intent(in) :: unit,v_list(:)\ncharacter(*),intent(in) :: iotype\ninteger,intent(out) :: iostat\ncharacter(*),intent(inout) :: iomsg\ncharacter(4) :: fmt\nfmt='(dt)'\nread(unit,fmt,iostat=iostat,iomsg=iomsg) dtv%p\nend subroutine\nsubroutine read_point")
        source=source.replace("read(unit,'(2i3)',", "character(5) :: fmt\nfmt='(2i3)'\nread(unit,fmt,")
        self.check(source,'7 9\n')

    def test_runtime_format_mismatch(self):
        self.check(self.main(self.source('(f3.0)'), 'read(u,"(dt)",iostat=status,iomsg=message) a\nprint *,status>0,len_trim(message)>0'), 'T T\n')

    def test_unhandled_runtime_error(self):
        self.check(self.main(self.source('invalid'), 'read(u,"(dt)") a'), error='format',reference=False)

    def test_format_expression_once(self):
        source=self.source('(2i3)', 'read(unit,make_format(),iostat=iostat,iomsg=iomsg) dtv%x,dtv%y')
        source=source.replace('implicit none', 'implicit none\ninteger :: calls=0',1)
        source=source.replace('end module', "function make_format() result(fmt)\ncharacter(5) :: fmt\ncalls=calls+1\nfmt='(2i3)'\nend function\nend module")
        self.check(self.main(source,'read(u,"(dt)") a\nprint *,calls,a%x,a%y'), '1 7 9\n')

    def test_array_items(self):
        source=self.source('(2i3)', 'read(unit,fmt,iostat=iostat,iomsg=iomsg) pair\ndtv%x=pair(1)\ndtv%y=pair(2)')
        source=source.replace('character(40) :: fmt', 'integer :: pair(2)\ncharacter(40) :: fmt')
        self.check(self.main(source,'read(u,"(dt)") a\nprint *,a%x,a%y'), '7 9\n')

    def test_independent_internal_child(self):
        source=self.source(transfer="local=' 42'\nread(local,fmt,iostat=iostat,iomsg=iomsg) dtv%y\nif(iostat/=0) return\nread(unit,fmt,iostat=iostat,iomsg=iomsg) dtv%x")
        source=source.replace('character(40) :: fmt', 'character(3) :: local\ncharacter(40) :: fmt')
        self.check(self.main(source,'read(u,"(dt)") a\nprint *,a%x,a%y'), '7 42\n')

    def test_independent_external_child(self):
        source=self.source(transfer="open(newunit=other,status='scratch')\nwrite(other,'(a)') ' 42'\nrewind(other)\nread(other,fmt,iostat=iostat,iomsg=iomsg) dtv%y\nclose(other)\nif(iostat/=0) return\nread(unit,fmt,iostat=iostat,iomsg=iomsg) dtv%x")
        source=source.replace('character(40) :: fmt', 'integer :: other\ncharacter(40) :: fmt')
        self.check(self.main(source,'read(u,"(dt)") a\nprint *,a%x,a%y'), '7 42\n')

    def test_mixed_intrinsic_fields(self):
        source=self.source('(a3,f5.2,l1,2i3)', 'read(unit,fmt,iostat=iostat,iomsg=iomsg) dtv%label,dtv%weight,dtv%yes,dtv%x,dtv%y')
        source=source.replace('integer :: x,y','integer :: x,y\ncharacter(3) :: label\nreal :: weight\nlogical :: yes')
        source=source.replace("'  7  9 99'", "'abc12.34T  7  9'")
        self.check(self.main(source,'read(u,"(dt)") a\nprint *,a%label,abs(a%weight-12.34)<0.001,a%yes,a%x,a%y'), 'abc T T 7 9\n')

    def test_eof(self):
        source=self.source('(i3)', 'read(unit,fmt,iostat=iostat,iomsg=iomsg) dtv%x,dtv%y')
        source=source.replace("write(u,'(a)') ' 11 13 99'", '')
        # Local gfortran loses the negative child status on parent return.
        self.check(self.main(source,'read(u,"(dt)",iostat=status,iomsg=message) a\nprint *,status<0,len_trim(message)>0'), 'T T\n',reference=False)

if __name__=='__main__':
    unittest.main()

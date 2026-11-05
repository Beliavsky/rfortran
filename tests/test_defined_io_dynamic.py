"""Runtime parent formats for defined input/output, interpreter and native builds."""
import unittest
from pathlib import Path
import test_deallocate
import test_defined_io as output
import test_defined_input_formats as input_formats

class DynamicDefinedIO(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_fixture(self):
        self.check((Path(__file__).parent / 'programs/xdefined_formatted_runtime.f90').read_text(),
                   '10 1 2 3 4 5.5\n')

    def test_output(self):
        source=output.DefinedIO().source("write(unit,'(i0)',iostat=iostat) dtv%x",'write(*,fmt) 7,a,9')
        source=source.replace('type(point) :: a,b', "type(point) :: a,b\ncharacter(40) :: fmt='(i0,1x,dt,1x,i0)'")
        self.check(source,'7 7 9\n')

    def test_input(self):
        source=input_formats.DefinedInputFormats().source(
            'integer :: n\ntype(point) :: a(2)\nreal :: r\ncharacter(40) :: fmt',
            'fmt="(i3,2dt,f5.1)"\nread(u,fmt) n,a,r\nprint *,n,a%x,a%y,r',
            [' 10  1  2  3  4  5.5'])
        self.check(source,'10 1 3 2 4 5.5\n')

    def out(self, main, child="write(unit,'(i0)',iostat=iostat) dtv%x", declarations=''):
        source=output.DefinedIO().source(child,main)
        return source.replace('type(point) :: a,b','type(point) :: a,b\ncharacter(100) :: fmt\n'+declarations)

    def test_output_metadata(self):
        self.check(self.out('fmt="(dt\'tag\'(3,2),1x,dt)"\nwrite(*,fmt) a,b',
            "if(size(v_list)>0) then\nwrite(unit,'(a,3(1x,i0))',iostat=iostat) iotype,size(v_list),v_list\nelse\nwrite(unit,'(a,1x,i0)',iostat=iostat) iotype,size(v_list)\nend if"),
            'DTtag 2 3 2 DT 0\n')

    def test_output_array_reversion(self):
        self.check(self.out('v(1)=a\nv(2)=b\nv(3)=a\nfmt="(2(dt,1x))"\nwrite(*,fmt) v',
            declarations='type(point) :: v(3)'), '7 9\n7\n')

    def test_output_unlimited_colon(self):
        self.check(self.out('v(1)=a\nv(2)=b\nfmt="(*(dt,:,1x))"\nwrite(*,fmt) v',
            declarations='type(point) :: v(2)'), '7 9\n')

    def test_output_invalid_format(self):
        self.check(self.out('fmt="(dt"\nwrite(*,fmt,iostat=status,iomsg=message) a\nprint *,status>0,len_trim(message)>0',
            declarations='integer :: status\ncharacter(100) :: message'),'T T\n',reference=False)

    def test_output_reader_error(self):
        self.check(self.out('fmt="(dt,i0)"\nwrite(*,fmt,iostat=status,iomsg=message) a,999\nprint *,status,trim(message)',
            "iostat=17\niomsg='failed'",'integer :: status\ncharacter(100) :: message'),'17 failed\n',reference=False)

    def test_input_reversion(self):
        self.check(input_formats.DefinedInputFormats().source(
            'type(point) :: a(3)\ncharacter(40) :: fmt',
            'fmt="(dt)"\nread(u,fmt) a\nprint *,a%x,a%y',
            ['  1  2 ignored','  3  4 ignored','  5  6 ignored']), '1 3 5 2 4 6\n')

    def test_input_character_logical(self):
        self.check(input_formats.DefinedInputFormats().source(
            'type(point) :: a\ncharacter(40) :: fmt\ncharacter(3) :: s\nlogical :: flag',
            'fmt="(a3,dt,l1)"\nread(u,fmt) s,a,flag\nprint *,s,a%x,a%y,flag',
            ['abc  1  2T']), 'abc 1 2 T\n')

    def test_input_failure(self):
        self.check(input_formats.DefinedInputFormats().source(
            'type(point) :: a\ncharacter(40) :: fmt\ninteger :: n',
            'a%x=-1\nfmt="(i3,dt)"\nread(u,fmt,iostat=status,iomsg=message) n,a\nprint *,status>0,a%x,len_trim(message)>0',
            ['bad  1  2']), 'T -1 T\n')

    def test_input_implied_decimal_spacing(self):
        self.check(input_formats.DefinedInputFormats().source(
            'type(point) :: a\ncharacter(40) :: fmt\nreal :: r',
            'fmt="(f4.2,2x,dt)"\nread(u,fmt) r,a\nprint *,r,a%x,a%y',
            [' 123xx  1  2']), '1.23 1 2\n')

    def test_output_assumed_length_format(self):
        source=self.out('fmt="(dt)"\ncall show(fmt,a)')
        source=source.replace('end module', """subroutine show(fmt,p)
character(*),intent(in) :: fmt
type(point),intent(in) :: p
write(*,fmt) p
end subroutine
end module""")
        self.check(source,'7\n')

    def test_output_format_evaluated_once(self):
        source=self.out('write(*,make_format()) a,b\nprint *,calls')
        source=source.replace('implicit none','implicit none\ninteger :: calls=0',1)
        source=source.replace('end module', """function make_format() result(fmt)
character(20) :: fmt
calls=calls+1
fmt='(dt,1x,dt)'
end function
end module""")
        self.check(source,'7 9\n1\n')

    def test_output_positioning_boundary(self):
        self.check(self.out('fmt="(t1,dt)"\nwrite(*,fmt,iostat=status,iomsg=message) a\nprint *,status>0,index(message,"T/TL")>0',
            declarations='integer :: status\ncharacter(100) :: message'),'T T\n',reference=False)

    def test_output_empty_array(self):
        self.check(self.out('fmt="(2x,dt)"\nwrite(*,fmt) values',declarations='type(point) :: values(0)'), '\n')

    def test_input_metadata(self):
        source=input_formats.DefinedInputFormats().source(
            'type(point) :: a\ncharacter(40) :: fmt',
            'fmt="(dt\'tag\'(3,2))"\nread(u,fmt) a\nprint *,a%x,a%y', ['  1  2'])
        source=source.replace("read(unit,'(2i3)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y",
            "read(unit,'(2i3)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y\nif(iotype=='DTtag') dtv%x=dtv%x+v_list(1)\ndtv%y=dtv%y+size(v_list)")
        self.check(source,'4 4\n')

    def test_input_radix_scale_blank(self):
        self.check(input_formats.DefinedInputFormats().source(
            'type(point) :: a\ncharacter(60) :: fmt\ninteger :: n\nreal :: r,q',
            'fmt="(z2,bz,f4.1,1p,f4.1,dt)"\nread(u,fmt) n,r,q,a\nprint *,n,r,q,a%x,a%y',
            ['FF1 2  123  1  2']), '255 102 1.23 1 2\n')

    def test_input_message_not_stale(self):
        source=input_formats.DefinedInputFormats().source(
            'type(point) :: a\ncharacter(40) :: fmt\ninteger :: n',
            'fmt="(dt,i3)"\nread(u,fmt,iostat=status,iomsg=message) a,n\nprint *,status>0,index(message,"success") == 0',
            ['  1  2bad'])
        source=source.replace("read(unit,'(2i3)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y",
            "read(unit,'(2i3)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y\nif(iostat==0) iomsg='success'")
        self.check(source,'T T\n',reference=False)

    def test_input_descriptor_mismatch(self):
        self.check(input_formats.DefinedInputFormats().source(
            'type(point) :: a\ncharacter(40) :: fmt\ninteger :: n',
            'n=-1\nfmt="(dt,i3)"\nread(u,fmt,iostat=status,iomsg=message) n,a\nprint *,status>0,n,len_trim(message)>0'),
            'T -1 T\n',reference=False)

    def test_input_eof_stops_later_items(self):
        self.check(input_formats.DefinedInputFormats().source(
            'type(point) :: a\ncharacter(40) :: fmt\ninteger :: n',
            'a%x=-1\nfmt="(i3,dt)"\nread(u,fmt,iostat=status,iomsg=message) n,a\nprint *,status<0,a%x,len_trim(message)>0',[]),
            'T -1 T\n')

    def test_input_reversed_sections(self):
        self.check(input_formats.DefinedInputFormats().source(
            'type(point) :: a(2)\ncharacter(40) :: fmt\ninteger :: n(3)',
            'n=-1\nfmt="(2i3,2dt)"\nread(u,fmt) n(3:1:-2),a(2:1:-1)\nprint *,n,a%x,a%y',
            [' 10 20  1  2  3  4']), '20 -1 10 3 1 4 2\n')

    def test_output_metadata_limit(self):
        fmt='(dt('+','.join(['1']*129)+'))'
        source=self.out('fmt="'+fmt+'"\nwrite(*,fmt,iostat=status,iomsg=message) a\nprint *,status>0,index(message,"128")>0',
            declarations='integer :: status\ncharacter(100) :: message').replace('character(100) :: fmt','character(1000) :: fmt')
        self.check(source,'T T\n',reference=False)

if __name__ == '__main__':
    unittest.main()

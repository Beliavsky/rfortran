"""Fortran 2023 named enum editing in interpreter and native O0/O2."""
import unittest
import subprocess
import tempfile
from pathlib import Path
from test_record_read import ROOT, build_commands
import test_fortran2023 as shared
from test_enum_types import ENUM


class EnumIO(unittest.TestCase):
    check = shared.Fortran2023.check
    reject = shared.Fortran2023.reject

    def test_example(self):
        self.check((ROOT/'tests/programs/xenum_io.f90').read_text(),
                   '  5  6  7\n  6  110  6  6 6\n  5  6  7\n')

    def test_runtime_bad_descriptor_and_read_without_iostat(self):
        cases = [(
            "type(season)::s\ncharacter(8)::fmt\ns=season(6)\nfmt='(f4.1)'\nprint fmt,s\n",
            '', 'descriptor does not match'),(
            "type(season)::s\nread(*,'(z2)')s\n", 'XX\n', 'formatted enumeration READ failed')]
        for body,data,message in cases:
            with self.subTest(body=body), tempfile.TemporaryDirectory() as tmp:
                work=Path(tmp)
                path=work/'enum_error.f90'
                path.write_text('implicit none\n'+ENUM+body+'end\n')
                for command in build_commands(path,work,cwd=work):
                    result=subprocess.run(command,input=data,cwd=work,capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)
                    if data:
                        self.assertIn('enum_error.f90:',result.stderr)

    def test_integer_output_descriptors(self):
        self.check("implicit none\n"+ENUM+'''type(season)::s
s=season(6)
print '(i3,1x,b4,1x,o2,1x,z2,1x,g0)',s,s,s,s,s
s=season(-1)
print '(i3,1x,z8.8)',s,s
end
''', '  6  110  6  6 6\n -1 FFFFFFFF\n')

    def test_runtime_formats_sections_and_implied_do(self):
        self.check("implicit none\n"+ENUM+'''type(season)::a(4)
integer::i
character(10)::fmt
fmt='(2i3)'
a=[season :: spring,season(6),summer,autumn]
print fmt,a(1:4:2)
print '(4i3)',(a(i),i=4,1,-1)
end
''','  5  7\n  8  7  6  5\n')

    def test_mixed_lists_and_format_reversion(self):
        self.check("implicit none\n"+ENUM+'''type(season)::a(3)
a=[season :: spring,season(6),summer]
print '(a,i3,1x,l1,1x,f3.1)', 'value=',a(2),.true.,1.5
print '(2i3)',a
end
''','value=  6 T 1.5\n  5  6\n  7\n')

    def test_read_integer_and_general_unnamed_values(self):
        self.check("implicit none\n"+ENUM+'''type(season)::a,b
integer::status
read(*,'(i4,g4.2)',iostat=status)a,b
print '(3i5)',status,a,b
end
''','    0  -99  123\n',data=' -99 123\n')

    def test_radix_input(self):
        self.check("implicit none\n"+ENUM+'''type(season)::a(3)
integer::status
read(*,'(b4,o3,z8)',iostat=status)a
print '(4i4)',status,a
end
''','   0  10  63  -1\n',data='1010 77FFFFFFFF\n')

    def test_internal_write_and_constructors(self):
        self.check("implicit none\n"+ENUM+'''type(season)::s
character(20)::buffer
s=season(6)
write(buffer,'(2i3)')[season :: spring,s]
print '(a)',trim(buffer)
end
''','  5  6\n')

    def test_enum_and_enumeration_in_same_list(self):
        self.check("implicit none\n"+ENUM+'''enumeration type :: colour
enumerator :: red,green
end enumeration type
type(season)::s
type(colour)::c
integer::status
read(*,'(i3,i2)',iostat=status)s,c
print '(3i4)',status,s,c
end
''','   0 -99   2\n',data='-99 2\n')

    def test_descriptor_and_storage_errors(self):
        for statement in ("print '(f5.1)',s", "print '(a)',s", "read(*,'(l2)')s"):
            with self.subTest(statement=statement):
                self.reject("implicit none\n"+ENUM+"type(season)::s\ns=season(6)\n"+statement+"\nend\n", 'descriptor')
        self.reject("implicit none\n"+ENUM+'''type(season),parameter::s=season(6)
read(*,'(i3)')s
end
''','writable storage')

    def test_radix_first_error_retained_and_following_read(self):
        self.check("implicit none\n"+ENUM+'''type(season)::a,b
integer::status
read(*,'(z2,z8)',iostat=status)a,b
print *,status>0
read(*,'(z2,z8)',iostat=status)a,b
print '(3i3)',status,a,b
end
''',' T\n  0  6 -1\n',data='XX00000007\n06FFFFFFFF\n')

    def test_bad_input_and_eof(self):
        for fmt,data in [('i12','2147483648\n'),('i4','oops\n'),('z9','100000000\n'),('b4','1020\n'),('z2','-1\n')]:
            with self.subTest(fmt=fmt,data=data):
                self.check("implicit none\n"+ENUM+'''type(season)::s
integer::status
read(*,'('''+fmt+''')',iostat=status)s
print *,status>0
end
''',' T\n',data=data)
        self.check("implicit none\n"+ENUM+'''type(season)::s
integer::status
read(*,'(i3)',iostat=status)s
print *,status<0
end
''',' T\n')

    def test_file_roundtrip(self):
        self.check("implicit none\n"+ENUM+'''type(season)::a,b
integer::u
a=season(-123)
open(newunit=u,status='scratch',action='readwrite')
write(u,'(z8.8)')a
rewind(u)
read(u,'(z8)')b
close(u)
print '(i5)',b
end
''',' -123\n')

    def test_shared_integer_radix_input_kinds_and_blanks(self):
        self.check('''implicit none
integer(kind=1)::a
integer(kind=2)::b
integer(kind=8)::c
integer::d,e,status
read(*,'(z2,z4,z16,bn,z4,bz,z4)',iostat=status)a,b,c,d,e
print *,status,a,b,c,d,e
end
''',' 0 -1 -1 -1 18 4098\n',data='FFFFFFFFFFFFFFFFFFFFFF1  21  2\n')


if __name__=='__main__':
    unittest.main()

"""Formatted enumeration I/O in the interpreter and native O0/O2 builds."""
from pathlib import Path
import subprocess
import tempfile
import unittest

import test_enumerations as enums
import test_fortran2023 as support
from test_record_read import ROOT, build_commands


class EnumerationIO(unittest.TestCase):
    check = support.Fortran2023.check
    reject = support.Fortran2023.reject

    def test_example(self):
        self.check((ROOT/'tests/programs/xenumeration_io.f90').read_text(),
                   'initial=2\ncolours: 1 2 3\nread=3\n', data='  3\n')

    def test_scalar_output_and_integer_descriptors(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour)::c
c=orange
print '(a,i3,l2)', 'colour=',c,c>red
print '(i1)',red,next(c),previous(c),huge(c),colour(2)
print '(b3,1x,o2,1x,z2)',green,orange,red
end
''', 'colour=  2 T\n1\n3\n1\n3\n2\n 11  2  1\n')

    def test_fixed_arrays_column_major_and_constructors(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour)::a(0:1,2)
a=red
a(1,1)=orange
a(0,2)=green
print '(4i2)',a
print '(3i2)',[green,red,orange]
print '(3i2)',[colour :: green,red,orange]
print '(i2)',[colour ::]
end
''', ' 1 2 3 1\n 3 1 2\n 3 1 2\n\n')

    def test_parameter_format(self):
        self.check('implicit none\n'+enums.ENUM+'''character(*),parameter::fmt='('// 'i2' //')'
type(colour)::c
read(*,fmt)c
print fmt,c
end
''', ' 2\n', data=' 2\n')

    def test_scalar_read_into_uninitialized_variable(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour)::c
read(*,'(i3)')c
print '(i2)',c
end
''', ' 3\n', data='  3\n')

    def test_array_read_and_mixed_items(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour)::a(2,2),c
integer::n,status
read(*,'(4i2)')a
print '(4i2)',a
read(*,'(i2,i3)',iostat=status)c,n
print *,status,int(c),n
end
''', ' 1 2 3 1\n 0 2 42\n', data=' 1 2 3 1\n 2 42\n')

    def test_input_designators_depend_on_earlier_items(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour)::c
integer::values(3)
c=red
values=0
read(*,'(i1,i1)')c,values(int(c))
print *,int(c),values
end
''',' 3 0 0 1\n',data='31\n')

    def test_invalid_ordinals_report_iostat(self):
        source='implicit none\n'+enums.ENUM+'''type(colour)::c
integer::status
c=orange
read(*,'(i4)',iostat=status)c
print *,status>0
read(*,'(i4)',iostat=status)c
print *,status,int(c)
end
'''
        for ordinal in ('0', '-1', '4', '9999', ''):
            with self.subTest(ordinal=ordinal):
                self.check(source, ' T\n 0 3\n', data=f'{ordinal:>4}\n   3\n')

    def test_bad_integer_does_not_lose_status_on_later_item(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour)::a,b
integer::status
read(*,'(2i2)',iostat=status)a,b
print *,status>0
read(*,'(2i2)',iostat=status)a,b
print *,status,int(a),int(b)
end
''', ' T\n 0 2 3\n', data='xx 2\n 2 3\n')

    def test_numeric_read_also_retains_first_error(self):
        self.check('''implicit none
integer::a,b,status
read(*,'(2i2)',iostat=status)a,b
print *,status>0
read(*,'(2i2)',iostat=status)a,b
print *,status,a,b
end
''',' T\n 0 2 3\n', data='xx 2\n 2 3\n')

    def test_eof_reports_iostat(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour)::c
integer::status
read(*,'(i2)',iostat=status)c
print *,status<0
end
''', ' T\n')

    def test_range_error_has_source_location(self):
        source='implicit none\n'+enums.ENUM+'''type(colour)::c
read(*,'(i3)')c
end
'''
        line=source.splitlines().index("read(*,'(i3)')c")+1
        with tempfile.TemporaryDirectory() as tmp:
            work=Path(tmp);path=work/'range.f90';path.write_text(source)
            for command in build_commands(path,work,cwd=work):
                result=subprocess.run(command,input='  0\n',cwd=work,capture_output=True,text=True,timeout=30)
                self.assertNotEqual(result.returncode,0,result.stdout+result.stderr)
                self.assertIn('ordinal outside 1..3',result.stderr)
                self.assertIn(f'range.f90:{line}',result.stderr)

    def test_wrong_descriptors_and_list_directed_rejected(self):
        for statement in ("print '(f4.1)',red", "print '(a)',red", "read(*,'(l2)')c"):
            with self.subTest(statement=statement):
                self.reject('implicit none\n'+enums.ENUM+'type(colour)::c\n'+statement+'\nend\n',
                            'require an I, B, O or Z')
        for statement in ('print *,red','write(*,*)red','read(*,*)c'):
            with self.subTest(statement=statement):
                self.reject('implicit none\n'+enums.ENUM+'type(colour)::c\n'+statement+'\nend\n','list-directed')

    def test_read_only_targets_rejected(self):
        self.reject('implicit none\n'+enums.ENUM+"read(*,'(i2)')red\nend\n",'writable storage')
        self.reject('module m\n'+enums.ENUM+'''contains
subroutine f(c)
type(colour),intent(in)::c
read(*,'(i2)')c
end subroutine
end module
end
''','INTENT(IN)')

    def test_iostat_type_and_small_kind(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour)::c
integer(kind=1)::status
read(*,'(i2)',iostat=status)c
print *,status>0
end
''',' T\n', data=' 0\n')
        self.reject('implicit none\n'+enums.ENUM+'''type(colour)::c
real::status
read(*,'(i2)',iostat=status)c
end
''','IOSTAT must be a writable scalar INTEGER')

    def test_file_write_and_read(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour)::a,b
integer::u
a=green
open(newunit=u,status='scratch',action='readwrite')
write(u,'(i3)')a
rewind(u)
read(u,'(i3)')b
close(u)
print '(i2)',b
end
''',' 3\n')

    def test_current_limits_have_explicit_diagnostics(self):
        prefix='implicit none\n'+enums.ENUM+'''type(colour)::a(3)
integer::i
character(4)::fmt
a=red
fmt='(i2)'
'''
        self.reject(prefix+'print fmt,a(1)\nend\n','constant CHARACTER format')
        self.reject(prefix+"print '(3i2)',(a(i),i=1,3)\nend\n",'scalar items or fixed whole arrays')
        self.reject(prefix+'print *,(a(i),i=1,3)\nend\n','list-directed')
        self.reject(prefix+"read(*,'(z2)')a(1)\nend\n",'only the I edit descriptor')
        self.reject(prefix+"read(*,'(2i2)')[a(1),a(2)]\nend\n",'writable storage')


if __name__=='__main__':
    unittest.main()

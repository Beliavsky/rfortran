"""Runtime READ formats in the interpreter and native O0/O2 code."""
import unittest
import test_record_read_iostat


class RuntimeReadFormat(unittest.TestCase):
    # Use the existing I/O test helpers without collecting its test methods.
    setUp = test_record_read_iostat.RecordReadIOStat.setUp
    commands = test_record_read_iostat.RecordReadIOStat.commands
    execute = test_record_read_iostat.RecordReadIOStat.execute
    def test_runtime_formats_and_aliasing(self):
        commands = self.commands('''implicit none
character(8)::fmt
character(5)::text
integer::status,i,lo,hi
fmt='(A)'
do i=1,2
read(*,trim(fmt),iostat=status) text
print *,status
if(status==0) print '(a)', '['//text//']'
fmt=' ( a ) '
end do
fmt='(a)'
read(*,fmt,iostat=status) fmt
print '(a)', '['//fmt//']'
read(*,'('//trim(fmt)//')') text
print '(a)', '['//text//']'
fmt='x(a)y';lo=2;hi=4
read(*,fmt(lo:hi)) text
print '(a)', '['//text//']'
read(*,'(a)',iostat=status) text
print *,status<0
end
''', reference=True)
        self.execute(commands,b'abcdef\nxy\na\nlast\nsub\n',
                     '0\n[abcde]\n0\n[xy   ]\n[a       ]\n[last ]\n[sub  ]\nT\n')

    def test_invalid_format_does_not_consume_input(self):
        commands=self.commands('''implicit none
character(8)::fmt
character(3)::text
integer::status
fmt='(i3)'
read(*,fmt,iostat=status) text
print *,status>0
fmt='(a)'
read(*,fmt,iostat=status) text
print *,status==0
print '(a)', '['//text//']'
end
''')
        self.execute(commands,b'ok\n','T\nT\n[ok ]\n')

    def test_optional_format_and_character_array_element(self):
        from test_file_io import literal
        path=self.directory/'records.txt'
        path.write_text('first\nsecond\n',encoding='ascii')
        commands=self.commands(f'''implicit none
character(4)::text(2)
character(8)::fmt
fmt='(a)'
open(20,file={literal(path)},status='old',action='read')
call read_strings(20,text,fmt)
print '(a)', '['//text(1)//']'
print '(a)', '['//text(2)//']'
rewind(20)
call read_strings(20,text)
print '(a)', '['//text(1)//']'
close(20)
contains
subroutine read_strings(unit,strings,fmt)
integer,intent(in)::unit
character(*),intent(out)::strings(:)
character(*),intent(in),optional::fmt
integer::i,status
do i=1,size(strings)
if(present(fmt)) then
read(unit,fmt,iostat=status) strings(i)
else
read(unit,'(a)',iostat=status) strings(i)
end if
if(status/=0) exit
end do
end subroutine
end
''',reference=True)
        self.execute(commands,b'','[firs]\n[seco]\n[firs]\n')

    def test_invalid_format_without_iostat(self):
        commands=self.commands('''character(8)::fmt
character(3)::text
fmt='bad'
read(*,fmt) text
end
''')
        self.execute(commands,b'ok\n',error='unsupported runtime format')

    def test_invalid_format_undefines_destination(self):
        commands=self.commands('''character(8)::fmt
character(3)::text
integer::status
fmt='bad';text='old'
read(*,fmt,iostat=status) text
print *,text
end
''')
        self.execute(commands,b'ok\n',error='uninitialized variable `text`')

    def test_noncharacter_format_is_rejected(self):
        source=self.directory/'bad.f90'
        source.write_text('integer::fmt\ncharacter::text\nfmt=1\nread(*,fmt) text\nend\n')
        import subprocess
        from test_record_read import COMPILER
        result=subprocess.run([COMPILER,'run',source],capture_output=True,text=True,timeout=30)
        self.assertNotEqual(result.returncode,0)
        self.assertIn('READ format must be scalar CHARACTER',result.stderr)


if __name__=='__main__':
    unittest.main(verbosity=2)

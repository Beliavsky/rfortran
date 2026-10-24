"""OPEN positioning: interpreter, native O0/O2, and gfortran reference."""
import tempfile
from pathlib import Path
import unittest

import test_deallocate
from test_file_io import literal
from test_record_read import ROOT


class OpenPosition(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_xacv_append(self):
        with tempfile.TemporaryDirectory(prefix='open append ') as tmp:
            program = (ROOT/'tests/programs/xacv_open_append.f90').read_text()
            program = program.replace("'rfort_open_append_test.tmp'", literal(Path(tmp)/'records.txt'))
            self.check(program, 'first\nsecond\n')

    def test_append_newunit_and_initial_position_only(self):
        with tempfile.TemporaryDirectory(prefix='append rewind ') as tmp:
            path = literal(Path(tmp)/'records.txt')
            self.check(f'''implicit none
integer::u,ios
character(20)::line
character(9)::action
open(newunit=u,file={path}, &
     & status='replace',action='write')
write(u,'(a)') 'first'
close(u)
open(newunit=u,file={path}, &
     & status='old',position='ApPeNd ',iostat=ios)
print *,ios
inquire(unit=u,action=action)
print *,action=='READWRITE'
write(u,'(a)') 'second'
rewind(u)
read(u,'(a)') line
print '(a)',trim(line)
read(u,'(a)') line
print '(a)',trim(line)
rewind(u)
write(u,'(a)') 'replacement'
rewind(u)
read(u,'(a)') line
print '(a)',trim(line)
close(u,status='delete')
end
''', '0\nT\nfirst\nsecond\nreplacement\n')

    def test_unknown_creates_missing_file(self):
        for newunit in (False, True):
            with self.subTest(newunit=newunit), tempfile.TemporaryDirectory(prefix='append create ') as tmp:
                path = literal(Path(tmp)/'created.txt')
                selector = 'newunit=u' if newunit else 'unit=u'
                self.check(f'''integer::u,ios
character(10)::line
u=22
open({selector},file={path}, &
     & position='append',action='write',iostat=ios)
print *,ios
write(u,'(a)') 'created'
close(u)
open(unit=22,file={path}, &
     & status='old',action='read')
read(22,'(a)') line
close(22,status='delete')
print '(a)',trim(line)
end
''', '0\ncreated\n')

    def test_replace_append_starts_empty(self):
        with tempfile.TemporaryDirectory(prefix='append replace ') as tmp:
            path = literal(Path(tmp)/'records.txt')
            self.check(f'''integer::u
character(20)::line
open(newunit=u,file={path}, &
     & status='replace')
write(u,'(a)') 'old contents'
close(u)
open(newunit=u,file={path}, &
     & status='replace',position='append')
write(u,'(a)') 'new contents'
rewind(u)
read(u,'(a)') line
print '(a)',trim(line)
close(u,status='delete')
end
''', 'new contents\n')

    def test_readonly_append_then_rewind(self):
        with tempfile.TemporaryDirectory(prefix='append read ') as tmp:
            path = Path(tmp)/'records.txt'
            path.write_text('first\nsecond\n')
            self.check(f'''integer::u,ios
character(20)::line
open(newunit=u,file={literal(path)}, &
     & status='old',action='read',position='append',iostat=ios)
print *,ios
read(u,'(a)',iostat=ios) line
print *,ios<0
rewind(u)
read(u,'(a)') line
print '(a)',trim(line)
close(u)
end
''', '0\nT\nfirst\n')

    def test_unformatted_stream_append(self):
        with tempfile.TemporaryDirectory(prefix='append stream ') as tmp:
            path = literal(Path(tmp)/'stream.bin')
            self.check(f'''integer::u,values(3)
open(newunit=u,file={path}, &
     & status='replace',access='stream',form='unformatted')
write(u) [10,20]
close(u)
open(newunit=u,file={path}, &
     & status='old',access='stream',form='unformatted',position='append')
write(u) 30
read(u,pos=1) values
print *,values
close(u,status='delete')
end
''', '10 20 30\n')

    def test_asis_rewind_and_scratch_positions(self):
        with tempfile.TemporaryDirectory(prefix='open position ') as tmp:
            path = Path(tmp)/'records.txt'
            path.write_text('first\nsecond\n')
            for position in ('asis', 'rewind'):
                with self.subTest(position=position):
                    self.check(f'''integer::u
character(20)::line
open(newunit=u,file={literal(path)}, &
     & status='old',position='{position}')
read(u,'(a)') line
print '(a)',trim(line)
close(u)
open(newunit=u,status='scratch',position='{position}')
write(u,'(a)') 'scratch'
rewind(u)
read(u,'(a)') line
print '(a)',trim(line)
close(u)
end
''', 'first\nscratch\n')
            self.check('''integer::u
character(10)::line
open(newunit=u,status='scratch',position='append')
write(u,'(a)') 'scratch'
rewind(u)
read(u,'(a)') line
print '(a)',trim(line)
close(u)
end
''', 'scratch\n')

    def test_failed_open_leaves_newunit_and_reports_message(self):
        with tempfile.TemporaryDirectory(prefix='append failure ') as tmp:
            path = literal(Path(tmp)/'missing.txt')
            self.check(f'''integer::u,ios
character(512)::message
u=123
open(newunit=u,file={path}, &
     & status='old',position='append',iostat=ios,iomsg=message)
print *,ios/=0,u==123,len_trim(message)>0
end
''', 'T T T\n')

    def test_validation(self):
        for controls, message in [
            ("file='x',position='invalid'", 'OPEN POSITION'),
            ("file='x',position=1", 'POSITION must currently be constant'),
            ("status='scratch',position='invalid'", 'OPEN POSITION'),
            ("file='x',position='append',position='rewind'", 'duplicate position'),
        ]:
            with self.subTest(controls=controls):
                self.reject(f'open(unit=20,{controls})\nend\n', message)


if __name__ == '__main__':
    unittest.main()

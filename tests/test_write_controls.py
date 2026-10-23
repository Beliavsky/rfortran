"""Named WRITE controls in interpreter/native O0/O2, compared with gfortran."""
import unittest
import test_deallocate


class WriteControls(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_runtime_format_and_reordered_controls(self):
        self.check('''character(20)::fmt
fmt='(a,1x,i0)'
write(*,fmt=fmt) 'value',7
write(fmt=trim(fmt),unit=6) 'next',8
write(advance='no',fmt='(a)',unit=6) 'joined'
write(6,fmt='(a)') 'end'
write(unit=*,fmt=*) 9
write(unit=*,fmt=*)
end
''','value 7\nnext 8\njoinedend\n9\n\n')

    def test_list_directed_implied_do(self):
        self.check('''integer::i
write(unit=*,fmt=*) (i*i,i=1,3)
end
''','1 4 9\n')

    def test_named_file_controls(self):
        self.check('''integer::unit
character(5)::text
open(newunit=unit,status='scratch',action='readwrite')
write(unit=unit,fmt='(a)') 'hello'
rewind(unit)
read(unit,'(a)') text
write(fmt='(a)',unit=6) text
close(unit)
end
''','hello\n')

    def test_invalid_controls(self):
        for statement,message in [
            ("write(*,fmt='(a)',fmt='(a)') 'x'",'duplicate WRITE control'),
            ("write(6,'(a)',unit=6) 'x'",'duplicate WRITE control'),
            ("write(unit=6,'(a)') 'x'",'positional WRITE controls'),
            ("write(fmt='(a)') 'x'",'WRITE requires UNIT'),
            ("write(unit=6) 'x'",'units 5 and 6 are formatted'),
            ("write(6,'(a)',advance=*) 'x'",'only WRITE UNIT and FMT'),
            ("write(6,'(a)',unknown=1) 'x'",'unsupported WRITE control'),
        ]:
            with self.subTest(statement=statement): self.reject(statement+'\nend\n',message)


if __name__=='__main__':
    unittest.main(verbosity=2)

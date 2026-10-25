"""Nonadvancing formatted external output, interpreter/native/reference."""
from pathlib import Path
import unittest
import test_deallocate as helpers


class FileNonadvancing(unittest.TestCase):
    check = helpers.Deallocate.check
    reject = helpers.Deallocate.reject

    def test_csv_records(self):
        source=(Path(__file__).parent/'programs/xfile_nonadvancing.f90').read_text(encoding='utf-8')
        self.check(source, ',SPY,TLT\n  20200101,  100.5000,  120.2500\n')

    def test_runtime_controls_and_constant_unit(self):
        self.check('''program main
implicit none
character(3)::adv
character(8)::fmt
character(30)::line
integer::i
open(unit=21,status='scratch',action='readwrite')
fmt='(i2)'
adv='NO'
write(unit=21,fmt=fmt,advance=adv) 1
write(21,'(a)',advance='no') ','
adv='Yes'
write(21,fmt,advance=adv) 2
write(21,'(3i2)',advance='no') (i,i=3,5)
write(21,'(a)') '!'
rewind(21)
read(21,'(a)') line
print '(a)',trim(line)
read(21,'(a)') line
print '(a)',trim(line)
close(21)
end program
''', ' 1, 2\n 3 4 5!\n')

    def test_empty_and_slash_records(self):
        self.check('''program main
implicit none
integer::u,i
character(30)::line
open(newunit=u,status='scratch',action='readwrite')
write(u,'(a)',advance='no') ''
write(u,'(a)',advance='no') 'a'
write(u,'(/,a)',advance='no') 'b'
write(u,'(a)',advance='yes') 'c'
write(u,'(a)') ''
rewind(u)
do i=1,3
read(u,'(a)') line
print '(a)', '|'//trim(line)//'|'
end do
close(u)
end program
''', '|a|\n|bc|\n||\n')

    def test_list_directed_advance_rejected(self):
        self.reject("open(unit=21,status='scratch')\nwrite(21,*,advance='no') 1\nend\n",
                    'ADVANCE requires an explicit format')


if __name__ == '__main__':
    unittest.main()

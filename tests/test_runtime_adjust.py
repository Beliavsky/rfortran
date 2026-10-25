"""ADJUSTL/ADJUSTR preserve the active length of runtime CHARACTER views."""
from pathlib import Path
import unittest
import test_deallocate as helpers


class RuntimeAdjust(unittest.TestCase):
    check = helpers.Deallocate.check

    def test_substring_fixture(self):
        source = (Path(__file__).parent / 'programs/xadjust_runtime.f90').read_text(encoding='utf-8')
        self.check(source, '|ab    |\n|    ab|\n6 6\n|ab|\n')

    def test_empty_blank_and_single_character(self):
        self.check('''program main
implicit none
character(5)::text='  x  '
integer::n
do n=0,5
print '(a)', '|'//adjustl(text(1:n))//'|', '|'//adjustr(text(1:n))//'|'
end do
end program
''', '||\n||\n| |\n| |\n|  |\n|  |\n|x  |\n|  x|\n|x   |\n|   x|\n|x    |\n|    x|\n')

    def test_allocatable_self_assignment(self):
        self.check('''program main
implicit none
character(:),allocatable::s
s='  ab  '
s=adjustl(string=s)
print '(a)', '|'//s//'|'
s=adjustr(s)
print '(a)', '|'//s//'|'
s=trim(adjustl(s))
print *,len(s)
print '(a)', '|'//s//'|'
s=''
s=adjustl(s)
s=adjustr(s)
print *,len(s)
end program
''', '|ab    |\n|    ab|\n2\n|ab|\n0\n')

    def test_token_assignment_and_nonblank_tab(self):
        self.check('''program main
implicit none
character(12)::line='  ab    cd  '
character(6)::tokens(2)
integer::i,first,last
do i=1,2
first=6*(i-1)+1
last=6*i
tokens(i)=adjustl(line(first:last))
end do
print '(a)', '|'//tokens(1)//'|', '|'//tokens(2)//'|'
line=' '//achar(9)//'x'
first=1
last=3
tokens(1)=adjustl(line(first:last))
print *,iachar(tokens(1)(1:1)),len(adjustl(line(first:last)))
end program
''', '|ab    |\n|cd    |\n9 3\n')


if __name__ == '__main__':
    unittest.main()

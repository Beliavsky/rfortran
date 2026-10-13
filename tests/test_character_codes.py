"""Character-code conversions and runtime substring assignment."""
import unittest
import test_deallocate
from test_record_read import ROOT

class CharacterCodes(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_original_program(self):
        self.check((ROOT/'tests/153_to_lower.f90').read_text(encoding='utf-8'),'jimbob\n')

    def test_runtime_input_case_conversion(self):
        source=(ROOT/'tests/153_to_lower.f90').read_text(encoding='utf-8')
        source=source.replace('print*,to_lower("JimBob")', '''character(12)::text
read(*,'(a)')text
print '(a)',"'"//to_lower(text)//"'"''')
        self.check(source,"'az09 !?-_   '\n",data='AZ09 !?-_\n')

    def test_empty_and_already_lowercase_inputs(self):
        source=(ROOT/'tests/153_to_lower.f90').read_text(encoding='utf-8')
        source=source.replace('print*,to_lower("JimBob")', '''print '(a)','['//to_lower('')//']'
print '(a)',to_lower('already lower 123!?')
print '(a)',to_lower('AZaz09')''')
        self.check(source,'[]\nalready lower 123!?\nazaz09\n')

    def test_character_code_roundtrip_and_kinds(self):
        self.check("""program main
integer::i,c
logical::good
integer(kind=1)::small
character(1)::text
small=65
text=char(small)
c=iachar(text)
print *,ichar(text),c,kind(c)
good=.true.
do i=1,127
good=good .and. iachar(achar(i))==i .and. ichar(char(i))==i
end do
print *,good
print '(a)',char(90)//achar(122)
end program main
""",'65 65 4\nT\nZz\n')

    def test_dynamic_assignment_padding_truncation_overlap_and_empty(self):
        self.check("""program main
character(6)::text
integer::lo,hi
text='abcdef'
read(*,*)lo
read(*,*)hi
text(lo:hi)='X'
print '(a)',"'"//text//"'"
text(lo:hi)='WXYZ'
print '(a)',"'"//text//"'"
text(lo:hi)=text(hi:hi+2)
print '(a)',"'"//text//"'"
text(hi:lo)='Q'
print '(a)',"'"//text//"'"
end program main
""", "'aX  ef'\n'aWXYef'\n'aYefef'\n'aYefef'\n",data='2\n4\n')

    def test_deferred_string_substring_assignment(self):
        self.check("""program main
character(len=:),allocatable::text
integer::lo,hi
text='abcdef'
read(*,*)lo
read(*,*)hi
text(lo:hi)='XY'
print '(a)',"'"//text//"'"
print *,len(text)
end program main
""", "'aXY ef'\n6\n",data='2\n4\n')

    def test_user_function_named_iachar(self):
        self.check("""print *,iachar(3)
contains
pure integer function iachar(n)
integer,intent(in)::n
iachar=n+1
end function iachar
end
""",'4\n')

    def test_invalid_codes_lengths_and_bounds(self):
        for code in (0,128,-1):
            with self.subTest(code=code):
                self.check('integer::code\ncharacter(1)::text\nread(*,*)code\ntext=achar(code)\nprint *,text\nend\n',
                           error='codes 1 through 127',data=str(code)+'\n')
        self.reject("print *,iachar('ab')\nend\n",'length one')
        self.reject("print *,char(1.0)\nend\n",'scalar INTEGER')
        self.reject("print *,iachar('a',8)\nend\n",'KIND is not yet supported')
        self.check("""character(3)::text
integer::lo,hi
text='abc'
read(*,*)lo
read(*,*)hi
print *,iachar(text(lo:hi))
end
""",error='length one',data='1\n2\n')
        self.check("""character(3)::text
integer::lo,hi
text='abc'
read(*,*)lo
read(*,*)hi
text(lo:hi)='x'
end
""",error='substring bounds',data='0\n1\n')

if __name__ == '__main__':
    unittest.main()

"""CHARACTER PARAMETER substrings and scalar INDEX in native code/interpreter."""
import unittest
import test_deallocate
from test_record_read import ROOT

class ParameterSubstrings(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_original_case_conversion(self):
        self.check((ROOT/'tests/157_parameter_substrings.f90').read_text(encoding='utf-8'),
                   'JIMBOB 09!\n')

    def test_runtime_and_elemental_case_conversion(self):
        source=(ROOT/'tests/157_parameter_substrings.f90').read_text(encoding='utf-8')
        source=source.replace("print '(a)', upper_case_str('JimBob 09!')",'''character(8)::text
read(*,'(a)')text
print '(a)',"'"//upper_case_str(text)//"'"
print '(a)',upper_case_str('')
print '(*(1x,a))',upper_case_str(['abc','xYz'])''')
        self.check(source,"'ABC!?   '\n\nABC XYZ\n",data='abc!?\n')

    def test_constant_folding_padding_and_module_imports(self):
        self.check("""module m
character(6),parameter::letters='abcd'
end module m
program main
use m,only:letters
character(*),parameter::piece=letters(2:4),empty=letters(7:6)
integer,parameter::width=len(letters(2:4))
character(2),parameter::words(2)=['ab','cd']
print '(a)',"'"//piece//"'"
print '(a)',"'"//letters(4:)//"'"
print '(a)',"'"//letters(:2)//"'"
print '(a)',"'"//empty//"'"
print *,width
print '(*(1x,a))',words(2:1:-1)
end program main
""", "'bcd'\n'd  '\n'ab'\n''\n3\ncd ab\n")

    def test_runtime_bounds_and_pure_function(self):
        self.check("""program main
character(*),parameter::alphabet='ABCDE'
integer::lo,hi
read(*,*)lo
read(*,*)hi
print '(a)',alphabet(lo:hi)
print '(a)',pick(lo)
contains
pure character(1) function pick(n)
integer,intent(in)::n
pick=alphabet(n:n)
end function pick
end program main
""",'BCD\nB\n',data='2\n4\n')

    def test_index_matches_missing_empty_and_trimmed_patterns(self):
        self.check("""program main
character(6)::text
character(2)::needle
read(*,'(a)')text
read(*,'(a)')needle
print *,index(text,needle),index(text,trim(needle))
print *,index(text,''),index('', ''),index('', 'x')
print *,index(text,'abcabcx'),index(text,'ab'),index(text,'bc'),index(text,'z')
print *,index(text(2:5),needle(1:1))
end program main
""",'0 1\n1 1 0\n0 1 2 0\n3\n',data='abcabc\na\n')

    def test_invalid_substrings_and_index_arguments(self):
        for expression,message in [('text(0:1)','outside CHARACTER length'),
                                   ('text(1:4)','outside CHARACTER length'),
                                   ('text(::2)','do not allow a stride')]:
            self.reject("character(*),parameter::text='abc'\nprint *,"+expression+'\nend\n',message)
        self.reject("character(*),parameter::text='abc'\ntext(1:1)='x'\nend\n",'cannot modify PARAMETER')
        self.reject("integer,parameter::n=3\nprint *,n(1:2)\nend\n",'not an array')
        self.reject("print *,index(1,'a')\nend\n",'scalar CHARACTER')
        self.reject("print *,index('abc','a',1)\nend\n",'scalar LOGICAL')
        self.check("""character(*),parameter::text='abc'
integer::lo
read(*,*)lo
print *,text(lo:lo)
end
""",error='substring bounds',data='4\n')

    def test_user_procedure_named_index(self):
        self.check("""print *,index(2)
contains
pure integer function index(n)
integer,intent(in)::n
index=n+1
end function index
end
""",'3\n')

if __name__ == '__main__':
    unittest.main()

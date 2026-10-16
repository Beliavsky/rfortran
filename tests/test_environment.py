"""Environment lookup in interpreter/native O0/O2 and gfortran."""
import os
import unittest
from unittest.mock import patch
import test_deallocate
from test_record_read import ROOT

class Environment(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_missing_fixture(self):
        with patch.dict(os.environ):
            os.environ.pop('RFORT_TEST_MISSING_71B8E979',None)
            self.check((ROOT/'tests/200_environment.f90').read_text(),'environment tests passed\n')

    def test_found_padding_truncation_and_length(self):
        with patch.dict(os.environ,{'RFORT_ENV_TEST':'hello world'}):
            self.check('''character(len=15)::value
character(len=3)::short
integer::n,s
call get_environment_variable('RFORT_ENV_TEST',value,n,s)
print *, '['//value//']',n,s
call get_environment_variable(length=n,status=s,name='RFORT_ENV_TEST',value=short)
print *, '['//short//']',n,s
call get_environment_variable('RFORT_ENV_TEST',length=n,status=s)
print *,n,s
end
''','[hello world    ] 11 0\n[hel] 11 -1\n11 0\n')

    def test_empty_and_zero_width(self):
        with patch.dict(os.environ,{'RFORT_ENV_TEST':'abc','RFORT_ENV_EMPTY':''}):
            self.check('''character(len=0)::zero
character(len=4)::value
integer::n,s
call get_environment_variable('RFORT_ENV_TEST',zero,n,s)
print *,n,s
call get_environment_variable('RFORT_ENV_EMPTY',value,n,s)
print *, '['//value//']',n,s
end
''','3 -1\n[    ] 0 '+('1' if os.name=='nt' else '0')+'\n')

    def test_trim_and_input_expressions(self):
        with patch.dict(os.environ,{'RFORT_ENV_TEST':'abc'}):
            self.check('''character(len=18)::name
character(len=4)::value
integer::n,s
logical::trim_name
name='RFORT_ENV_TEST'
trim_name=.true.
call get_environment_variable(trim_name=trim_name,name=name,value=value,length=n,status=s)
print *,value,n,s
trim_name=.false.
call get_environment_variable(name,value,n,s,trim_name)
print *,n,s
call get_environment_variable(trim(name)//'',value,n,s)
print *,value,n,s
end
''','abc 3 0\n0 1\nabc 3 0\n')

    def test_subroutine_and_array_element_outputs(self):
        with patch.dict(os.environ,{'RFORT_ENV_TEST':'abc'}):
            self.check('''character(len=20)::value
integer::n(2),s(2)
value='RFORT_ENV_TEST'
call get_environment_variable('RFORT_ENV_TEST',value,n(1),s(2))
print *,trim(value),n(1),s(2)
call query(value)
print *,trim(value)
contains
subroutine query(text)
character(len=*),intent(out)::text
call get_environment_variable('RFORT_ENV_TEST',value=text)
end subroutine
end
''','abc 3 0\nabc\n')

    def test_character_array_output(self):
        with patch.dict(os.environ,{'RFORT_ENV_TEST':'abc'}):
            self.check("""character(len=5)::values(2)
integer::n,s
call get_environment_variable('RFORT_ENV_TEST',values(2),n,s)
print *, '['//values(2)//']',n,s
end
""",'[abc  ] 3 0\n')

    def test_validation(self):
        for source,message in [
            ('call get_environment_variable(1)','NAME requires scalar CHARACTER'),
            ('integer::x\ncall get_environment_variable("X",x)','VALUE requires CHARACTER'),
            ('real::n\ncall get_environment_variable("X",length=n)','default INTEGER'),
            ('integer::x\ncall get_environment_variable("X",trim_name=x)','TRIM_NAME requires scalar LOGICAL'),
            ('call get_environment_variable("X",value="literal")','writable output'),
        ]:
            self.reject(source+'\nend',message)
        self.reject('''program main
contains
pure subroutine f(x)
character(len=*),intent(out)::x
call get_environment_variable('X',x)
end subroutine
end program
''','PURE')

if __name__=='__main__': unittest.main()

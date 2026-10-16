"""GET_COMMAND_ARGUMENT in interpreter, native O0/O2, and gfortran."""
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
import test_deallocate
from test_record_read import ROOT,COMPILER,build_commands
from run_ofort_compat import equivalent

class CommandArgument(unittest.TestCase):
    reject=test_deallocate.Deallocate.reject

    def check(self,source,expected,arguments=(),data='',reference=True):
        with tempfile.TemporaryDirectory(prefix='command-argument-') as tmp:
            directory=Path(tmp);path=directory/'main.f90';path.write_text(source)
            commands=build_commands(path,directory)
            commands[0]+= ['--',*arguments]
            commands[1:]=[cmd+list(arguments) for cmd in commands[1:]]
            if reference and shutil.which('gfortran'):
                exe=directory/'reference.exe'
                result=subprocess.run(['gfortran','-std=f2018','-fcheck=all',path,'-o',exe],capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                commands.append([exe,*arguments])
            for cmd in commands:
                with self.subTest(command=cmd):
                    result=subprocess.run(cmd,input=data,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stderr,'')
                    self.assertTrue(equivalent(result.stdout,expected),result.stdout)

    def test_fixture(self):
        self.check((ROOT/'tests/202_command_argument.f90').read_text(),'command argument tests passed\n')

    def test_arguments_keywords_empty_and_spaces(self):
        self.check('''character(30)::value
integer::n,s,i
do i=1,4
call get_command_argument(status=s,number=i,length=n,value=value)
print *, '['//trim(value)//']',n,s
end do
end
''','[hello world] 11 0\n[] 0 0\n[-Wall] 5 0\n[a"b] 3 0\n',arguments=['hello world','','-Wall','a"b'])

    def test_truncation_and_length_only(self):
        self.check("""character(2)::short
integer::n,s
call get_command_argument(1,short,n,s)
print *,short,n,s
call get_command_argument(1,length=n,status=s)
print *,n,s
end
""",'ab 3 -1\n3 0\n',arguments=['abc'])

    def test_zero_width_buffer(self):
        # GNU's runtime reports a processor error for a zero-length VALUE.
        # rfort supports it: nonempty values truncate; empty values fit.
        self.check("""character(0)::zero
integer::n,s
call get_command_argument(1,zero,n,s)
print *,n,s
call get_command_argument(2,zero,n,s)
print *,n,s
end
""",'3 -1\n0 0\n',arguments=['abc',''],reference=False)

    def test_missing_and_negative(self):
        self.check('''character(8)::value
integer::n,s,i
i=-1
call get_command_argument(i,value,n,s)
print *,value==' ',n,s>0
call get_command_argument(999999,value,n,s)
print *,value==' ',n,s>0
end
''','T 0 T\nT 0 T\n')

    def test_array_elements_and_assumed_length_output(self):
        self.check('''character(8)::values(2),text
integer::lengths(2),statuses(2),indices(2)
indices=[1,2]
call get_command_argument(indices(1),values(2),lengths(2),statuses(1))
print *,trim(values(2)),lengths(2),statuses(1)
call capture(text)
print *,trim(text)
contains
subroutine capture(value)
character(*),intent(out)::value
call get_command_argument(1,value=value)
end subroutine
end
''','abc 3 0\nabc\n',arguments=['abc'])

    def test_number_expression_evaluated_once(self):
        self.check('''character(4)::text
integer::s
call get_command_argument(next_number()+0,text,status=s)
print *,trim(text),s
contains
integer function next_number()
integer::n
read(*,*) n
next_number=n
end function
end
''','abc 0\n',arguments=['abc'],data='1\n')

    def test_intrinsic_integer_expressions(self):
        self.check("""integer::indices(2)
character(4)::text
indices=[1,2]
call get_command_argument(min(indices(1),2),text)
print *,trim(text)
call get_command_argument(merge(size(indices),1,.true.),text)
print *,trim(text)
call get_command_argument(select_index(indices),text)
print *,trim(text)
call get_command_argument(count(indices>0,kind=4),text)
print *,trim(text)
contains
integer function select_index(values)
integer,intent(in)::values(:)
select_index=values(1)
end function
end
""",'abc\nxyz\nabc\nxyz\n',arguments=['abc','xyz'])

    def test_run_option_forwards_arguments(self):
        with tempfile.TemporaryDirectory(prefix='command-cli-') as tmp:
            path=Path(tmp)/'main.f90';exe=Path(tmp)/'native.exe'
            path.write_text('character(30)::text\ncall get_command_argument(1,text)\nprint *,trim(text)\nend\n')
            result=subprocess.run([COMPILER,path,'--run','-o',exe,'--','hello world'],capture_output=True,text=True,timeout=60)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertTrue(equivalent(result.stdout,'hello world\n'),result.stdout)

    def test_validation(self):
        for declarations,call,message in [
            ('','call get_command_argument(1.0)','NUMBER requires scalar default INTEGER'),
            ('integer(8)::n\nn=1','call get_command_argument(n)','NUMBER requires scalar default INTEGER'),
            ('integer::n(2)\nn=1','call get_command_argument(min(1,n))','NUMBER requires scalar default INTEGER'),
            ('character(3)::x\nx="abc"','call get_command_argument(len_trim(x,kind=8))','NUMBER requires scalar default INTEGER'),
            ('integer::x','call get_command_argument(1,value=x)','VALUE requires CHARACTER'),
            ('real::n','call get_command_argument(1,length=n)','default INTEGER'),
            ('','call get_command_argument(1,value="literal")','writable output'),
        ]:
            self.reject(declarations+'\n'+call+'\nend',message)
        self.reject('pure subroutine s(x)\ncharacter(*),intent(out)::x\ncall get_command_argument(0,x)\nend subroutine\n','PURE')

if __name__=='__main__': unittest.main()

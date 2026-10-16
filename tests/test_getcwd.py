"""GNU GETCWD checks, including source locations for initialization errors."""
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
import test_deallocate
from test_record_read import ROOT, COMPILER
from run_ofort_compat import equivalent

class Getcwd(unittest.TestCase):
    reject=test_deallocate.Deallocate.reject

    def check(self,source,expected):
        test_deallocate.Deallocate.check(self,source,expected,reference=False)
        if shutil.which('gfortran'):
            with tempfile.TemporaryDirectory(prefix='getcwd-reference-') as tmp:
                path=Path(tmp)/'main.f90';exe=Path(tmp)/'reference.exe'
                path.write_text(source)
                result=subprocess.run(['gfortran','-std=gnu','-fcheck=all',path,'-o',exe],capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                result=subprocess.run([exe],capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(result.stderr,'')
                self.assertTrue(equivalent(result.stdout,expected),result.stdout)

    def test_fixture(self):
        self.check((ROOT/'tests/201_getcwd.f90').read_text(),'getcwd tests passed\n')

    def test_original_repro(self):
        self.check((ROOT/'xgetcwd.f90').read_text(),'T\n')

    def test_padding_and_output_dummies(self):
        self.check('''character(len=4096)::path
character(len=4096)::paths(2)
integer(kind=8)::statuses(2)
call capture(path)
call getcwd(paths(2),statuses(1))
print *,path==paths(2),statuses(1)==0
print *,len_trim(path)>0,path(len_trim(path)+1:)==' '
contains
subroutine capture(value)
character(len=*),intent(out)::value
call getcwd(value)
end subroutine
end
''','T T\nT T\n')

    def test_too_short_and_zero_width(self):
        self.check('''character(len=0)::zero
character(len=1)::short
integer::s
call getcwd(zero,s)
print *,s/=0
call getcwd(short,s)
print *,s==34,short==' '
call getcwd(short)
print *,short==' '
end
''','T\nT T\nT\n')

    def test_documented_keyword_interface(self):
        # GCC currently registers the first keyword as COUNT despite documenting C.
        # Check rfort's documented C= form separately from the GNU reference tests.
        test_deallocate.Deallocate.check(self,"""character(4096)::path
integer::s
call getcwd(status=s,c=path)
print *,s==0,len_trim(path)>0
end
""",'T T\n',reference=False)

    def test_validation(self):
        for text,message in [
            ('integer::x\ncall getcwd(x)','CWD requires CHARACTER'),
            ('character(20)::x(2)\ncall getcwd(x)','writable scalar'),
            ('character(20)::x\nreal::s\ncall getcwd(x,s)','STATUS requires INTEGER'),
            ('call getcwd("literal")','writable scalar'),
            ('pure subroutine f(x)\ncharacter(*),intent(out)::x\ncall getcwd(x)\nend subroutine','PURE'),
            ('subroutine f(x)\ncharacter(*),intent(in)::x\ncall getcwd(x)\nend subroutine','writable scalar'),
        ]:
            self.reject(text+'\nend',message)

    def test_unset_variables_still_fail_at_the_source_line(self):
        with tempfile.TemporaryDirectory(prefix='getcwd-unset-') as tmp:
            path=Path(tmp)/'unset.f90'
            path.write_text('implicit none\ninteger::n\ncharacter(4096)::path\ncall getcwd(path)\nprint *,n\nend\n')
            result=subprocess.run([COMPILER,'run',path],capture_output=True,text=True,timeout=30)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('read of uninitialized variable `n`',result.stderr)
            self.assertIn('unset.f90:5:',result.stderr)
            self.assertIn('print *,n',result.stderr)

    def test_separate_source_diagnostic(self):
        with tempfile.TemporaryDirectory(prefix='getcwd-origins-') as tmp:
            module=Path(tmp)/'bad_module.f90';main=Path(tmp)/'main.f90'
            module.write_text('module m\ncontains\ninteger function f()\ninteger::n\nf=n\nend function\nend module\n')
            main.write_text('use m\nprint *,f()\nend\n')
            result=subprocess.run([COMPILER,module,main,'-o',Path(tmp)/'bad.exe'],capture_output=True,text=True,timeout=60)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('bad_module.f90:5:',result.stderr)
            self.assertIn('read of uninitialized variable `n`',result.stderr)

if __name__=='__main__': unittest.main()

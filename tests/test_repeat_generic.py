"""Intrinsic REPEAT fallback, user overloads, and native CHARACTER results."""
import subprocess
import tempfile
from pathlib import Path
import unittest
import test_deallocate
from test_record_read import ROOT, COMPILER

MODULE = (ROOT/'xrepeat_intrinsic.f90').read_text(encoding='utf-8').split('program xrepeat_intrinsic')[0]


class RepeatGeneric(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_reproducer(self):
        self.check((ROOT/'xrepeat_intrinsic.f90').read_text(encoding='utf-8'), '[000]\n')

    def test_custom_and_intrinsic_keywords(self):
        # A loop avoids the separate dynamic CHARACTER constructor limitation.
        module = MODULE.replace('yy = [(xx, i=1,n)]', 'do i=1,n\nyy(i)=xx\nend do')
        self.check(module+"""use repeat_mod
print *,repeat(2,'xy')
print *,repeat(xx='ab',n=3)
print *, '['//repeat(ncopies=3,string='0')//']'
print *, '['//repeat(string='ab',ncopies=2)//']'
end
""", 'xyxy\nababab\n[000]\n[abab]\n')

    def test_variables_trim_empty_and_assignment(self):
        self.check(MODULE+"""use repeat_mod
integer,parameter::n=3
character(len=4)::s
character(len=8)::t
s='ab'
print *, '['//repeat(trim(s),n)//']'
print *, '['//repeat(s,0)//']', '['//repeat('',3)//']'
t=repeat(s,2)
print *, '['//t//']'
s=repeat(s,2)
print *, '['//s//']'
end
""", '[ababab]\n[][]\n[ab  ab  ]\n[ab  ]\n')

    def test_specific_procedure_takes_precedence(self):
        self.check("""module m
interface repeat
module procedure custom
end interface
contains
integer function custom(string,ncopies) result(n)
character(len=*),intent(in)::string
integer,intent(in)::ncopies
n=len(string)+ncopies+10
end function
end module
use m
print *,repeat('abc',2),repeat(ncopies=3,string='ab')
end
""", '15 15\n')

    def test_invalid_intrinsic_fallback(self):
        for expression, message in [
            ("repeat('x',2.0)", 'no matching specific'),
            ("repeat(['x'],2)", 'no matching specific'),
            ("repeat(string='x',wrong=2)", 'unknown keyword'),
            ("repeat(string='x')", 'missing required'),
            ("repeat('x',string='x',ncopies=2)", 'duplicate argument'),
            ("repeat(string='x',2)", 'positional argument after'),
        ]:
            with self.subTest(expression=expression):
                self.reject(MODULE+'use repeat_mod\nprint *,'+expression+'\nend\n',message)

    def test_native_runtime_count_reports_limitation(self):
        with tempfile.TemporaryDirectory(prefix='repeat-runtime-') as tmp:
            source=Path(tmp)/'example.f90'
            source.write_text(MODULE+"use repeat_mod\ninteger::n\nread(*,*) n\nprint *,repeat('x',n)\nend\n",encoding='utf-8')
            result=subprocess.run([COMPILER,'emit-obj',source,Path(tmp)/'example.obj'],capture_output=True,text=True,timeout=60)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('constant INTEGER NCOPIES',result.stderr)


if __name__ == '__main__':
    unittest.main(verbosity=2)

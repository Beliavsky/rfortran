"""Intrinsic purity regressions: interpreter, native O0/O2 and gfortran."""
import subprocess
import tempfile
import unittest
from pathlib import Path
import test_deallocate
from test_record_read import COMPILER

class PureIntrinsics(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_pure_intrinsic_execution(self):
        for expression,expected in [('sum(pack(a,a>0))','4'),
                                    ('sum(cshift(a,1))','3'),
                                    ('sum(reshape(a,[3]))','3'),
                                    ('sum(eoshift(a,1))','2')]:
            with self.subTest(expression=expression):
                self.check(f"""program main
implicit none
integer::a(3)
a=[1,-1,3]
print *,f(a)
contains
pure integer function f(a) result(j)
integer,intent(in)::a(3)
j={expression}
end function
end program
""",expected+'\n')
        self.check("""program main
implicit none
print *,f(4.0)
contains
pure real function f(x) result(y)
real,intent(in)::x
y=sqrt(x)
end function
end program
""",'2\n')

    def test_associated_pure_pointer_inquiry(self):
        source="""pure logical function f(p) result(j)
integer,pointer,intent(in)::p(:)
j=associated(p)
end function
"""
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'example.f90'
            path.write_text(source,encoding='utf-8')
            result=subprocess.run([COMPILER,'check',path],capture_output=True,text=True,encoding='utf-8')
            self.assertEqual(result.returncode,0,result.stderr)

    def test_impure_and_unknown_calls_still_rejected(self):
        for call in ['call random_number(x)','call random_seed()',
                     'call cpu_time(x)','call system_clock(count=n)',
                     'call date_and_time()', 'call unknown()']:
            with self.subTest(call=call):
                self.reject('pure subroutine f()\nreal::x\ninteger::n\n'+call+'\nend subroutine\n','PURE')
        self.reject("""pure integer function f() result(j)
j=merge(1,2,.true.)
end function
integer function merge(a,b,mask) result(j)
integer,intent(in)::a,b
logical,intent(in)::mask
j=a
end function
""",'impure or unknown procedure `merge`')

if __name__ == '__main__': unittest.main()

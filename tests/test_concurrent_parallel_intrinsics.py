"""Audited scalar intrinsics in parallel loops and PURE/SIMPLE callees."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER, ROOT
import test_concurrent_parallel as support

# Nonconstant inputs keep the intrinsic calls present through source proof.
GROUPS = {
    'arithmetic': [
        ('mod(-i,7)', '-mod(i,7)'),
        ('modulo(-i,7)', 'modulo(7-mod(i,7),7)'),
        ('sign(i,-i)', '-i'), ('dim(i,i+1)', '0'), ('dim(i+1,i)', '1'),
        ('mod(-x,0.25d0)', '-mod(x,0.25d0)'),
        ('modulo(-x,0.25d0)', 'modulo(0.25d0-mod(x,0.25d0),0.25d0)'),
        ('sign(x,-x)', '-x'), ('dim(x+1.0d0,x)', '1.0d0'),
        ('dprod(real(x),2.0)', '2.0d0*dble(real(x))'),
    ],
    'rounding': [
        ('nint(x+0.5d0)', '1'), ('nint(-x-0.5d0,kind=8)', '-1'),
        ('floor(-x,kind=8)', '-1'), ('ceiling(x)', '1'),
        ('aint(-x-1.0d0)', '-1.0d0'), ('anint(-x-0.5d0)', '-1.0d0'),
        ('aint(x+1.0d0,kind=4)', '1.0'), ('anint(x+0.5d0,kind=8)', '1.0d0'),
    ],
    'math': [
        ('log10(10.0d0**x)', 'x'), ('tan(atan(x))', 'x'),
        ('sin(asin(x))', 'x'), ('cos(acos(x))', 'x'),
        ('atan(x,1.0d0)', 'atan2(x,1.0d0)'),
        ('sinh(asinh(x))', 'x'), ('cosh(acosh(x+1.0d0))', 'x+1.0d0'),
        ('tanh(atanh(x))', 'x'), ('hypot(3.0d0*x,4.0d0*x)', '5.0d0*x'),
        ('log(exp(x))', 'x'), ('sqrt(x*x)', 'x'),
    ],
    'inquiries': [
        ('huge(i)', '2147483647'), ('digits(i)', '31'), ('bit_size(i)', '32'),
        ('kind(x)', '8'), ('digits(x)', '53'), ('radix(x)', '2'),
        ('precision(x)', '15'), ('range(x)', '307'),
        ('minexponent(x)', '-1021'), ('maxexponent(x)', '1024'),
        ('epsilon(x)*4503599627370496.0d0', '1.0d0'),
        ('huge(x)/huge(1.0d0)', '1.0d0'), ('tiny(x)/tiny(1.0d0)', '1.0d0'),
    ],
}


def program(cases, context):
    body=['x=dble(modulo(i,8000)+1)/10000.0d0', 'r=0']
    for expression, expected in cases:
        body.append(f'if (abs(dble({expression})-dble({expected}))>1.0d-6) r=r+1')
    code='\n'.join(body)
    declarations='integer::i,r,y(9000)\ndouble precision::x'
    if context == 'direct':
        loop=code+'\ny(i)=r'
        tail=''
        locality=' local(x,r)'
    elif context == 'function':
        loop='y(i)=probe(i)'
        tail=f'contains\npure integer function probe(i) result(r)\ninteger,intent(in)::i\ndouble precision::x\n{code}\nend function\n'
        locality=''
    else:
        loop='call probe(i,y(i))'
        tail=f'contains\nsimple subroutine probe(i,r)\ninteger,intent(in)::i\ninteger,intent(out)::r\ndouble precision::x\n{code}\nend subroutine\n'
        locality=''
    return f"implicit none\n{declarations}\ndo concurrent(i=1:9000){locality}\n{loop}\nend do\nif(any(y/=0)) stop 1\nprint *, 'PASS'\n{tail}end\n"


class ParallelIntrinsics(unittest.TestCase):
    build_run=support.ParallelConcurrent.build_run

    def test_intrinsics_in_direct_loops_and_callees(self):
        for group, cases in GROUPS.items():
            for context in ('direct', 'function', 'subroutine'):
                with self.subTest(group=group,context=context):
                    self.build_run(program(cases,context),'PASS')

    def test_standalone_example(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_intrinsics.f90').read_text(),'PASS')

    def test_single_precision_math(self):
        for context in ('direct','function','subroutine'):
            source=program(GROUPS['math'],context).replace('double precision::x','real::x').replace('d0','e0')
            self.build_run(source,'PASS')

    def test_serial_and_unchecked_results(self):
        for context in ('direct','function','subroutine'):
            source=program(GROUPS['rounding']+GROUPS['math'],context)
            self.build_run(source,'PASS',parallel=False,threads=(1,))
            self.build_run(source,'PASS',checks=False)

    def test_rounding_overflow_still_reports_runtime_error(self):
        for name in ('floor','ceiling'):
            source=f"""implicit none
integer::i,y(9000)
double precision::x
x=1.0d30
do concurrent(i=1:9000)
 y(i)={name}(x+dble(i))
end do
print *,y(1)
end
"""
            with tempfile.TemporaryDirectory(prefix='rfort-parallel-intrinsic-') as tmp:
                path=Path(tmp)/'case.f90';path.write_text(source)
                exe=Path(tmp)/'case.exe'
                for optimization in ('0','2'):
                    build=subprocess.run([str(COMPILER),'-O',optimization,'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                    self.assertEqual(build.returncode,0,build.stderr)
                    self.assertIn('native parallel worker emitted',build.stderr)
                    for threads in (1,2,4):
                        env=os.environ.copy();env['RFORT_NUM_THREADS']=str(threads)
                        result=subprocess.run([str(exe)],env=env,capture_output=True,text=True,timeout=30)
                        self.assertNotEqual(result.returncode,0)
                        self.assertIn(f'{name.upper()} result is outside INTEGER range',result.stderr)

    def test_unaudited_pure_intrinsic_remains_serial(self):
        for context in ('direct','function','subroutine'):
            self.build_run(program([('log_gamma(x+1.0d0)','log_gamma(x+1.0d0)')],context),
                           'PASS',reason='serial fallback')

    def test_impure_intrinsic_remains_serial(self):
        self.build_run("""implicit none
integer::i,y(9000)
real::x
do concurrent(i=1:9000) local(x)
 call random_number(x)
 y(i)=floor(x)
end do
print *,sum(y)
end
""",'0',reason='serial fallback')

    def test_user_procedure_named_like_intrinsic(self):
        self.build_run("""implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
 y(i)=floor(i)
end do
print *,y(1),y(9000)
contains
integer function floor(i) result(r)
integer,intent(in)::i
r=i+7
end function
end
""",'8 9007',reason='serial fallback')


if __name__=='__main__':
    unittest.main()

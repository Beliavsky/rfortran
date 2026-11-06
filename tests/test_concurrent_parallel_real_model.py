"""Real model intrinsics in parallel loops and PURE/SIMPLE callees."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import test_concurrent_parallel as support
from test_record_read import COMPILER, ROOT


def program(kind,context,edges=False):
    declarations=f'real({kind})::x,t,sub,z\ninteger::p'
    body=f"""p=modulo(i,20)-10
x=scale(real(2*modulo(i,2)-1,{kind})*0.75_{kind},p)
r=0
"""
    cases=[
        (f'scale(i=int(2,1),x=x)', 'x*4'),
        ('fraction(x=x)', f'sign(0.75_{kind},x)'), ('exponent(x=x)', 'p'),
        ('set_exponent(i=3_8,x=x)', f'sign(6.0_{kind},x)'),
        ('spacing(x=x)', 'scale(epsilon(x),p-1)'),
        ('rrspacing(x=x)', f'scale(0.75_{kind},digits(x))'),
        ('kind(scale(x,1))',str(kind)), ('kind(fraction(x))',str(kind)),
        ('kind(set_exponent(x,1))',str(kind)), ('kind(nearest(x,1.0))',str(kind)),
        ('kind(spacing(x))',str(kind)), ('kind(rrspacing(x))',str(kind)),
        ('kind(exponent(x))','4'),
    ]
    conditions=[f'nearest(s=1.0_8,x=x)>x',f'nearest(s=-1.0_4,x=x)<x']
    if edges:
        declarations=f'real({kind})::x,t,sub,z'
        body=f"""t=tiny(0.0_{kind})
sub=nearest(0.0_{kind},1.0_{kind})
x=sign(sub,real(2*modulo(i,2)-1,{kind}))
z=-0.0_{kind}
r=0
"""
        cases=[
            ('scale(x,digits(x)-1)','sign(t,x)'),
            ('scale(x,-1)',f'sign(0.0_{kind},x)'),
            ('fraction(x)',f'sign(0.5_{kind},x)'),
            ('exponent(x)','minexponent(x)-digits(x)+1'),
            ('set_exponent(x,1)',f'sign(1.0_{kind},x)'),
            ('spacing(x)','t'), ('rrspacing(x)',f'scale(0.5_{kind},digits(x))'),
            (f'nearest(x,sign(1.0_{kind},x))','2*x'),
            (f'nearest(x,-sign(1.0_{kind},x))',f'sign(0.0_{kind},x)'),
            ('nearest(z,1.0)','sub'), ('nearest(z,-1.0)','-sub'),
            (f'sign(1.0_{kind},scale(z,1))',f'-1.0_{kind}'),
            (f'sign(1.0_{kind},fraction(z))',f'-1.0_{kind}'),
            (f'sign(1.0_{kind},set_exponent(z,1))',f'-1.0_{kind}'),
            ('exponent(z)','0'), ('spacing(z)','t'), ('rrspacing(z)','0'),
        ]
        conditions=[]
    for expression,expected in cases:
        body+=f'if ({expression}/={expected}) r=r+1\n'
    for condition in conditions:
        body+=f'if (.not.({condition})) r=r+1\n'
    if context=='direct':
        header=f'integer::i,r,y(9000)\n{declarations}'
        local='x,t,sub,z,r' if edges else 'x,p,r'
        # Do not assign unused variables: they are not part of this test.
        loop=body+'y(i)=r';locality=f' local({local})';tail=''
    elif context=='function':
        header='integer::i,y(9000)';locality='';loop='y(i)=probe(i)'
        tail=f'contains\npure integer function probe(i) result(r)\ninteger,intent(in)::i\n{declarations}\n{body}end function\n'
    else:
        header='integer::i,y(9000)';locality='';loop='call probe(i,y(i))'
        tail=f'contains\nsimple subroutine probe(i,r)\ninteger,intent(in)::i\ninteger,intent(out)::r\n{declarations}\n{body}end subroutine\n'
    return f"implicit none\n{header}\ndo concurrent(i=1:9000){locality}\n{loop}\nend do\nif(any(y/=0)) stop 1\nprint *, 'PASS'\n{tail}end\n"


def nonfinite_program(kind,context,expression):
    if context=='direct':
        loop='y(i)='+expression.replace('x','a(i)');tail=''
    elif context=='function':
        loop='y(i)=probe(a(i))'
        tail=f'contains\npure real({kind}) function probe(x) result(y)\nreal({kind}),intent(in)::x\ny={expression}\nend function\n'
    else:
        loop='call probe(a(i),y(i))'
        tail=f'contains\nsimple subroutine probe(x,y)\nreal({kind}),intent(in)::x\nreal({kind}),intent(out)::y\ny={expression}\nend subroutine\n'
    scaled=expression.startswith('scale')
    expected='a(i)' if scaled else f'sign(huge(0.0_{kind}),a(i))'
    return f"""use,intrinsic::ieee_arithmetic,only:ieee_value,ieee_quiet_nan,ieee_positive_inf,ieee_is_nan
implicit none
integer::i
real({kind})::a(9000),y(9000)
do i=1,9000
 if(modulo(i,3)==0) then
  a(i)=ieee_value(0.0_{kind},ieee_quiet_nan)
 else
  a(i)=sign(ieee_value(0.0_{kind},ieee_positive_inf),real(2*modulo(i,2)-1,{kind}))
 end if
end do
do concurrent(i=1:9000)
{loop}
end do
do i=1,9000
 if(modulo(i,3)==0) then
  if(.not.ieee_is_nan(y(i))) stop 1
 else
  if(y(i)/={expected}) stop 2
 end if
end do
print *, 'PASS'
{tail}end
"""


class ParallelRealModel(unittest.TestCase):
    def build_run(self,*args,**kwargs):
        # A module cache in the caller's directory may come from another build.
        # Keep these intrinsic-module tests independent of that cache.
        previous=Path.cwd()
        with tempfile.TemporaryDirectory(prefix='rfort-model-module-cache-') as tmp:
            try:
                os.chdir(tmp)
                support.ParallelConcurrent.build_run(self,*args,**kwargs)
            finally:
                os.chdir(previous)

    def test_all_intrinsics_kinds_keywords_and_call_contexts(self):
        for kind in (4,8):
            for context in ('direct','function','subroutine'):
                with self.subTest(kind=kind,context=context):
                    self.build_run(program(kind,context),'PASS')

    def test_subnormals_and_signed_zero(self):
        for kind in (4,8):
            for context in ('direct','function','subroutine'):
                with self.subTest(kind=kind,context=context):
                    self.build_run(program(kind,context,True),'PASS')

    def test_infinities_and_nan_propagation(self):
        for kind in (4,8):
            for context in ('direct','function','subroutine'):
                for expression in ('scale(x,1)',f'nearest(x,-sign(1.0_{kind},x))'):
                    with self.subTest(kind=kind,context=context,expression=expression):
                        self.build_run(nonfinite_program(kind,context,expression),'PASS')

    def test_native_real_and_integer_results_with_runtime_arrays(self):
        for kind in (4,8):
            self.build_run(f"""implicit none
integer::i,n
real({kind}),allocatable::a(:),y(:)
integer,allocatable::e(:)
n=9000
allocate(a(-2:n-3),y(-2:n-3),e(-2:n-3))
a=6
do concurrent(i=-2:n-3)
 y(i)=half(a(i))
 e(i)=power(a(i))
end do
if(any(y/=3).or.any(e/=3)) stop 1
print *, 'PASS'
contains
pure real({kind}) function half(x) result(y)
real({kind}),intent(in)::x
y=scale(fraction(x),exponent(x)-1)
end function
simple integer function power(x) result(e)
real({kind}),intent(in)::x
e=exponent(x)
end function
end
""",'PASS')

    def test_serial_and_unchecked_results(self):
        for context in ('direct','function','subroutine'):
            source=program(8,context,True)
            self.build_run(source,'PASS',parallel=False,threads=(1,))
            self.build_run(source,'PASS',checks=False)

    @unittest.skipUnless(shutil.which('gfortran'),'gfortran unavailable')
    def test_reference_results(self):
        with tempfile.TemporaryDirectory(prefix='rfort-real-model-reference-') as tmp:
            tmp=Path(tmp);path=tmp/'case.f90';exe=tmp/'case.exe'
            for kind in (4,8):
                for edges in (False,True):
                    path.write_text(program(kind,'function',edges))
                    build=subprocess.run(['gfortran','-std=f2018','-O2','-fcheck=all',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=60)
                    self.assertEqual(build.returncode,0,build.stderr)
                    result=subprocess.run([str(exe)],capture_output=True,text=True,timeout=30)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stdout.split(),['PASS'])

    def test_nearest_zero_direction_runtime_error(self):
        for kind in (4,8):
            for context in ('direct','function','subroutine'):
                for direction in ('0.0','-0.0'):
                    if context=='direct':
                        loop='y(i)=nearest(x,s)';tail=''
                    elif context=='function':
                        loop='y(i)=probe(x,s)'
                        tail=f'contains\npure real({kind}) function probe(x,s) result(y)\nreal({kind}),intent(in)::x,s\ny=nearest(s=s,x=x)\nend function\n'
                    else:
                        loop='call probe(x,s,y(i))'
                        tail=f'contains\nsimple subroutine probe(x,s,y)\nreal({kind}),intent(in)::x,s\nreal({kind}),intent(out)::y\ny=nearest(s=s,x=x)\nend subroutine\n'
                    source=f'implicit none\ninteger::i\nreal({kind})::x,s,y(9000)\nx=1\ns={direction}\ndo concurrent(i=1:9000)\n{loop}\nend do\nprint *,y(1)\n{tail}end\n'
                    with self.subTest(kind=kind,context=context,direction=direction),tempfile.TemporaryDirectory(prefix='rfort-nearest-error-') as tmp:
                        path=Path(tmp)/'case.f90';path.write_text(source);exe=Path(tmp)/'case.exe'
                        for opt in ('0','2'):
                            command=[str(COMPILER),'-O',opt,'--parallel','--parallel-report']
                            if context=='subroutine':command+=['--nocheck']
                            build=subprocess.run(command+[str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                            self.assertEqual(build.returncode,0,build.stderr)
                            self.assertIn('native parallel worker emitted',build.stderr)
                            self.assertNotIn('serial fallback',build.stderr)
                            for threads in (1,2,4):
                                env=os.environ.copy();env['RFORT_NUM_THREADS']=str(threads)
                                result=subprocess.run([str(exe)],env=env,capture_output=True,text=True,timeout=30)
                                self.assertNotEqual(result.returncode,0)
                                self.assertIn('NEAREST S must not be zero',result.stderr)

    def test_invalid_calls(self):
        for expression,diagnostic in [('scale(1,2)','X must be REAL'),('scale(1.0,2.0)','I must be INTEGER'),('nearest(1.0,0.0)','NEAREST S must not be zero')]:
            with self.subTest(expression=expression),tempfile.TemporaryDirectory(prefix='rfort-real-model-invalid-') as tmp:
                path=Path(tmp)/'case.f90';exe=Path(tmp)/'case.exe'
                path.write_text(f'implicit none\ninteger::i\nreal::a(9000)\ndo concurrent(i=1:9000)\na(i)={expression}\nend do\nend\n')
                result=subprocess.run([str(COMPILER),'--parallel',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(diagnostic,result.stderr)

    def test_shadowed_intrinsic_remains_serial(self):
        self.build_run("""implicit none
integer::i,a(9000)
do concurrent(i=1:9000)
 a(i)=scale(i,1)
end do
print *,a(1),a(9000)
contains
pure integer function scale(x,n) result(y)
integer,intent(in)::x,n
y=x+n+10
end function
end
""",'12 9011',reason='serial fallback')

    def test_standalone_example(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_real_model.f90').read_text(),'PASS')


if __name__=='__main__':
    unittest.main()

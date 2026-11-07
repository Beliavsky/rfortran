"""COMPLEX component storage and calls in native DO CONCURRENT workers."""
import os
from pathlib import Path
import tempfile
import shutil
import subprocess
import unittest
import test_concurrent_parallel as support
from test_record_read import ROOT,COMPILER


def program(kind=4, context='direct'):
    decl=f'complex({kind})::t,z\ncomplex({kind}),allocatable::a(:),y(:)'
    body=f't=x+cmplx(real(i,{kind}),-real(i,{kind}),kind={kind})\nr=t*z+conjg(t)'
    tail=''
    if context=='direct':
        loop=body.replace('t=x+','t=a(i-3)+').replace('r=','y(i)=');local=' local(t)'
    elif context=='function':
        loop='y(i)=probe(i,a(i-3),z)';local=''
        tail=f'contains\npure function probe(i,x,z) result(r)\ninteger,intent(in)::i\ncomplex({kind}),intent(in)::x,z\ncomplex({kind})::r,t\n{body}\nend function\n'
    elif context=='array':
        loop='y(i)=probe(i,a,z)';local=''
        tail=f'contains\npure function probe(i,x,z) result(r)\ninteger,intent(in)::i\ncomplex({kind}),intent(in)::x(:),z\ncomplex({kind})::r,t\n{body.replace("t=x+","t=x(1)+")}\nend function\n'
    else:
        loop='call probe(i,a(i-3),z,y(i))';local=''
        tail=f'contains\nsimple subroutine probe(i,x,z,r)\ninteger,intent(in)::i\ncomplex({kind}),intent(in)::x,z\ncomplex({kind}),intent(out)::r\ncomplex({kind})::t\n{body}\nend subroutine\n'
    return f"""implicit none
integer::i,n
{decl}
n=9000
allocate(a(-2:n-3),y(n))
a=cmplx(3.0_{kind},4.0_{kind},kind={kind})
z=cmplx(1.0_{kind},2.0_{kind},kind={kind})
do concurrent(i=1:n){local}
{loop}
end do
do i=1,n
if(abs(real(y(i),{kind})-real(4*i-2,{kind}))>0.001_{kind}) stop 1
if(abs(aimag(y(i))-real(2*i+6,{kind}))>0.001_{kind}) stop 2
end do
print *, 'PASS'
{tail}end
"""


class ParallelComplex(unittest.TestCase):
    def build_run(self,*args,**kwargs):
        previous=Path.cwd()
        with tempfile.TemporaryDirectory(prefix='rfort-complex-module-cache-') as tmp:
            try:
                os.chdir(tmp)
                support.ParallelConcurrent.build_run(self,*args,**kwargs)
            finally:os.chdir(previous)

    def test_complex_kinds_and_calls(self):
        for kind in (4,8):
            for context in ('direct','function','array','subroutine'):
                with self.subTest(kind=kind,context=context):
                    self.build_run(program(kind,context),'PASS')

    def test_standalone_example(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_complex.f90').read_text(),'PASS')

    def test_written_input_still_uses_serial_fallback(self):
        self.build_run("""implicit none
integer::i
complex(8)::a(9000)
a=(3d0,4d0)
do concurrent(i=1:9000)
a(i)=a(1)
end do
if(any(abs(a-(3d0,4d0))>1d-10)) stop 1
print *, 'PASS'
end
""",'PASS',reason='not confined to the current iteration',threads=(4,))


    def test_local_init_arithmetic_and_inquiries(self):
        for kind in (4,8):
            with self.subTest(kind=kind):
                self.build_run(f"""implicit none
integer::i,n
complex({kind})::z,t,y(9000)
real({kind})::lengths(9000)
n=9000
z=cmplx(1.0_{kind},2.0_{kind},kind={kind})
t=z
do concurrent(i=1:n) local_init(t)
t=t+cmplx(real(modulo(i,10),{kind}),0.0_{kind},kind={kind})
y(i)=(t*z+conjg(t))/z-t
lengths(i)=abs(t)
end do
do i=1,n
if(abs(y(i)-conjg(z+real(modulo(i,10),{kind}))/z)>0.0001_{kind}) stop 1
if(abs(lengths(i)-sqrt(real(1+modulo(i,10),{kind})**2+4))>0.0001_{kind}) stop 2
end do
if(abs(t-z)>0.0001_{kind}) stop 3
print *, 'PASS'
end
""",'PASS')

    def test_multidimensional_arrays_and_sections(self):
        self.build_run("""implicit none
integer::i,j,n
complex(8),allocatable::x(:,:),y(:,:)
n=100
allocate(x(-1:n-2,3:n+2),y(n,n))
x=(3d0,4d0)
do concurrent(j=1:n,i=1:n)
y(i,j)=shift(x(i-2,j+2),i+j)
end do
do j=1,n
do i=1,n
if(abs(y(i,j)-cmplx(3d0+i+j,4d0,kind=8))>1d-10) stop 1
end do
end do
print *, 'PASS'
contains
pure complex(8) function shift(x,n) result(y)
complex(8),intent(in)::x
integer,intent(in)::n
y=x+n
end function
end
""",'PASS')

    def test_strided_inputs_and_complex_array_result(self):
        source="""implicit none
integer::i
complex(8)::x(6),y(9000)
x=(3d0,4d0)
do concurrent(i=1:9000)
y(i)=probe(x(1:6:2),i)
end do
do i=1,9000
if(abs(y(i)-cmplx(3d0+i,-4d0,kind=8))>1d-10) stop 1
end do
print *, 'PASS'
contains
pure complex(8) function probe(x,i) result(r)
complex(8),intent(in)::x(:)
integer,intent(in)::i
complex(8)::work(3)
work=copy(x)
r=conjg(work(2))+i
end function
pure function copy(x) result(y)
complex(8),intent(in)::x(:)
complex(8)::y(size(x))
y=x
end function
end
"""
        self.build_run(source,'PASS')
        self.build_run(source.replace('complex(8)::work(3)','complex(8),allocatable::work(:)\nallocate(work(size(x)))'),'PASS')
        dynamic=source.replace('complex(8)::x(6),y(9000)',
            'complex(8),allocatable::x(:)\ncomplex(8)::y(9000)\ninteger::n\nn=6\nallocate(x(n))')
        dynamic=dynamic.replace('probe(x(1:6:2),i)','probe(x,i)').replace('complex(8)::work(3)',
            'complex(8),allocatable::work(:)\nallocate(work(size(x)))')
        self.build_run(dynamic,'PASS')

    def test_serial_and_unchecked(self):
        for context in ('direct','function','array','subroutine'):
            self.build_run(program(8,context),'PASS',parallel=False,threads=(1,))
        self.build_run(program(4,'subroutine'),'PASS',checks=False)

    def test_complex_reduction(self):
        self.build_run("""implicit none
integer::i
complex(8)::total
total=(0d0,0d0)
do concurrent(i=1:9000) reduce(+:total)
total=total+(1d0,2d0)
end do
if(abs(total-(9000d0,18000d0))>1d-10) stop 1
print *, 'PASS'
end
""",'PASS')

    def test_runtime_initialization_errors_still_apply(self):
        source="""implicit none
integer::i
complex(8)::t,y(9000)
do concurrent(i=1:9000) local(t)
if(modulo(i,2)==0) t=(3d0,4d0)
y(i)=t
end do
end
"""
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'unset.f90';exe=Path(tmp)/'unset.exe';path.write_text(source)
            for optimization in ('0','2'):
                result=subprocess.run([str(COMPILER),'-O',optimization,'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertIn('native parallel worker emitted',result.stderr)
                env=dict(os.environ,RFORT_NUM_THREADS='4')
                result=subprocess.run([str(exe)],env=env,capture_output=True,text=True,timeout=30)
                self.assertNotEqual(result.returncode,0)
                self.assertIn('uninitialized',result.stderr.lower())

    @unittest.skipUnless(shutil.which('gfortran'),'gfortran is not available')
    def test_gfortran_reference(self):
        for kind in (4,8):
            with self.subTest(kind=kind),tempfile.TemporaryDirectory() as tmp:
                path=Path(tmp)/'reference.f90';exe=Path(tmp)/'reference.exe'
                path.write_text(program(kind).replace('do concurrent(i=1:n) local(t)','do i=1,n'))
                result=subprocess.run(['gfortran','-std=f2018','-fcheck=all',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                result=subprocess.run([str(exe)],capture_output=True,text=True,timeout=30)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(result.stdout.split(),['PASS'])


if __name__=='__main__':unittest.main()

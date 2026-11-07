"""Audited real special functions and scalar MERGE in parallel workers."""
import os
from pathlib import Path
import subprocess
import shutil
import tempfile
import unittest
import test_concurrent_parallel as support
import test_deallocate
from test_record_read import COMPILER,ROOT


def program(kind,context):
    decl=f'real({kind})::x,tol\ninteger::n'
    body=f"""x=1.0_{kind}+real(modulo(i,100),{kind})/100.0_{kind}
tol={'0.00002' if kind==4 else '0.000000000002'}_{kind}
n=modulo(i,3)
r=0
if(abs(erf(x=x)+erfc(x=x)-1.0_{kind})>tol) r=r+1
if(abs(erfc_scaled(x=x)-exp(x*x)*erfc(x))>tol) r=r+1
if(abs(gamma(x=x+1)-x*gamma(x))>tol) r=r+1
if(abs(bessel_jn(n=n,x=x)- &
 merge(bessel_j0(x),merge(bessel_j1(x),2*bessel_j1(x)/x-bessel_j0(x),n==1),n==0))>tol) r=r+1
if(abs(bessel_yn(x=x,n=n)- &
 merge(bessel_y0(x),merge(bessel_y1(x),2*bessel_y1(x)/x-bessel_y0(x),n==1),n==0))>tol) r=r+1
if(abs(bessel_jn(0,x)-bessel_j0(x))>tol) r=r+1
if(abs(bessel_yn(1,x)-bessel_y1(x))>tol) r=r+1
if(kind(bessel_jn(n,x))/={kind}) r=r+1
if(kind(bessel_yn(n,x))/={kind}) r=r+1
if(kind(erfc_scaled(x))/={kind}) r=r+1
if(kind(gamma(x))/={kind}) r=r+1
if(merge(tsource=i,fsource=-i,mask=modulo(i,2)==0)/=i*(2*modulo(i+1,2)-1)) r=r+1
if(merge(.true.,.false.,modulo(i,2)==0).neqv.(modulo(i,2)==0)) r=r+1
if(merge(x,-x,modulo(i,2)==0)/=(x*real(2*modulo(i+1,2)-1,{kind}))) r=r+1
"""
    if context=='direct':
        header=f'integer::i,r,y(9000)\n{decl}';local=' local(x,tol,n,r)';tail='';loop=body+'y(i)=r'
    elif context=='function':
        header='integer::i,y(9000)';local='';loop='y(i)=probe(i)'
        tail=f'contains\npure integer function probe(i) result(r)\ninteger,intent(in)::i\n{decl}\n{body}end function\n'
    else:
        header='integer::i,y(9000)';local='';loop='call probe(i,y(i))'
        tail=f'contains\nsimple subroutine probe(i,r)\ninteger,intent(in)::i\ninteger,intent(out)::r\n{decl}\n{body}end subroutine\n'
    return f"implicit none\n{header}\ndo concurrent(i=1:9000){local}\n{loop}\nend do\nif(any(y/=0)) stop 1\nprint *, 'PASS'\n{tail}end\n"


class ParallelSpecial(unittest.TestCase):
    def build_run(self,*args,**kwargs):
        previous=Path.cwd()
        with tempfile.TemporaryDirectory(prefix='rfort-special-module-cache-') as tmp:
            try:
                os.chdir(tmp)
                support.ParallelConcurrent.build_run(self,*args,**kwargs)
            finally:os.chdir(previous)

    check=test_deallocate.Deallocate.check

    def test_functions_kinds_and_call_contexts(self):
        for kind in (4,8):
            for context in ('direct','function','subroutine'):
                with self.subTest(kind=kind,context=context):
                    self.build_run(program(kind,context),'PASS')

    @unittest.skipUnless(shutil.which('gfortran'),'gfortran is not available')
    def test_gfortran_reference(self):
        for kind in (4,8):
            with self.subTest(kind=kind),tempfile.TemporaryDirectory() as tmp:
                path=Path(tmp)/'reference.f90';exe=Path(tmp)/'reference.exe'
                path.write_text(program(kind,'direct').replace('do concurrent(i=1:9000) local(x,tol,n,r)','do i=1,9000'))
                result=subprocess.run(['gfortran','-std=f2018','-fcheck=all',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                result=subprocess.run([str(exe)],capture_output=True,text=True,timeout=30)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(result.stdout.split(),['PASS'])

    def test_large_bessel_order_keeps_integer_precision(self):
        self.check("""implicit none
integer(8)::n
real(8)::x,a,b
n=16777217_8
x=1d9
a=bessel_jn(n,x)
b=bessel_jn(n,-x)
print *,abs(a+b)<1d-12,abs(a)>1d-9
print *,kind(bessel_jn(n,1.0)),kind(bessel_yn(n,1.0))
end
""",'T T\n4 4\n',compile_timeout=90)

    def test_nonfinite_error_function_arguments(self):
        self.build_run("""use,intrinsic::ieee_arithmetic,only:ieee_value,ieee_positive_inf,ieee_quiet_nan,ieee_is_nan
implicit none
integer::i
real(8)::x(9000),a(9000),b(9000),c(9000)
x(1:3000)=ieee_value(0d0,ieee_positive_inf)
x(3001:6000)=-ieee_value(0d0,ieee_positive_inf)
x(6001:9000)=ieee_value(0d0,ieee_quiet_nan)
do concurrent(i=1:9000)
a(i)=erf(x(i))
b(i)=erfc(x(i))
c(i)=erfc_scaled(x(i))
end do
if(any(a(1:3000)/=1d0).or.any(b(1:3000)/=0d0).or.any(c(1:3000)/=0d0)) stop 1
if(any(a(3001:6000)/=-1d0).or.any(b(3001:6000)/=2d0)) stop 2
if(any(c(3001:6000)/=x(1))) stop 3
if(.not.all(ieee_is_nan(a(6001:9000)))) stop 4
if(.not.all(ieee_is_nan(b(6001:9000)))) stop 5
if(.not.all(ieee_is_nan(c(6001:9000)))) stop 6
print *, 'PASS'
end
""",'PASS')

    def test_serial_and_unchecked(self):
        self.build_run(program(8,'function'),'PASS',parallel=False,threads=(1,))
        self.build_run(program(4,'subroutine'),'PASS',checks=False)

    def test_scaled_error_function_values_and_single_evaluation(self):
        self.check("""implicit none
integer::calls=0
real(8)::x,y
y=erfc_scaled(next())
print '(f18.12,i3)',y,calls
x=26d0
print '(f18.12)',erfc_scaled(x)
x=100d0
print '(f18.12)',erfc_scaled(x)
x=1d200
y=erfc_scaled(x)
print *,y>0,y<1d-199
x=-0d0
print *,erfc_scaled(x)==1
contains
real(8) function next()
calls=calls+1
next=1d0
end function
end
""",'0.427583576156 1\n0.021683584851\n0.005641613783\nT T\nT\n')

    def test_merge_integer_kinds_and_logical_function_result(self):
        for kind in (1,2,4,8):
            with self.subTest(kind=kind):
                self.build_run(f"""implicit none
integer::i
integer({kind})::y(9000)
logical::a(9000)
do concurrent(i=1:9000)
y(i)=merge(int(7,{kind}),int(-3,{kind}),modulo(i,2)==0)
a(i)=choose(i)
end do
if(any(y(2:9000:2)/=7).or.any(y(1:8999:2)/=-3)) stop 1
if(kind(merge(int(7,{kind}),int(-3,{kind}),.true.))/={kind}) stop 3
if(.not.all(a)) stop 2
print *, 'PASS'
contains
pure logical function choose(i)
integer,intent(in)::i
choose=merge(.true.,.false.,i>0)
end function
end
""",'PASS')

    def test_scaled_tail_in_parallel(self):
        for kind in (4,8):
            self.build_run(f"""implicit none
integer::i
real({kind})::x,y(9000)
x=huge(x)
do concurrent(i=1:9000)
y(i)=erfc_scaled(x)
end do
if(any(y<=0).or.any(y>=tiny(x))) stop 1
print *, 'PASS'
end
""",'PASS')

    def test_log_gamma_keeps_serial_fallback(self):
        self.build_run("""implicit none
integer::i
real(8)::y(9000)
do concurrent(i=1:9000)
y(i)=log_gamma(3d0)
end do
if(any(abs(y-log(2d0))>1d-12)) stop 1
print *, 'PASS'
end
""",'PASS',reason='serial fallback',threads=(4,))

    def test_shadowed_names_keep_serial_fallback(self):
        self.build_run("""implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=erf(i)
end do
if(any(y/=7)) stop 1
print *, 'PASS'
contains
pure integer function erf(i)
integer,intent(in)::i
erf=7
end function
end
""",'PASS',reason='serial fallback',threads=(4,))

    def test_merge_kind_inquiry_does_not_read_sources(self):
        self.check("""implicit none
integer(1)::a,b
real(8)::x,y
logical::p,q
complex(kind=8)::u,v
character(3)::s='abc',t='def'
print *,kind(merge(mask=.true.,fsource=b,tsource=a))
print *,kind(merge(x,y,.false.)),kind(merge(p,q,.true.))
print *,kind(merge(u,v,.true.)),kind(merge(s,t,.false.))
end
""",'1\n8 4\n8 1\n')

    def test_user_merge_kind_keeps_its_interface(self):
        self.check("""implicit none
print *,kind(merge(1d0,2d0,.true.)),kind(int(merge(1d0,2d0,.false.)))
contains
pure integer function merge(a,b,m)
real(8),intent(in)::a,b
logical,intent(in)::m
merge=7
end function
end
""",'4 4\n')

    def test_fixture(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_special.f90').read_text(),'PASS')

if __name__=='__main__':unittest.main()

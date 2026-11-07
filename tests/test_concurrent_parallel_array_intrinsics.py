"""Array reductions/searches in native parallel loops and PURE/SIMPLE callees."""
import os
from pathlib import Path
import tempfile
import shutil
import subprocess
import unittest
import test_concurrent_parallel as support
from test_record_read import ROOT
import test_deallocate


def program(group, context='direct', kind=4):
    if group == 'bits':
        decl=f'integer({kind}), allocatable::a(:)\nlogical,allocatable::m(:)'
        init=f'allocate(a(-2:1),m(4))\na=[1_{kind},3_{kind},5_{kind},7_{kind}]\nm=[.true.,.false.,.true.,.false.]'
        dummy=f'integer({kind}),intent(in)::a(:)\nlogical,intent(in)::m(:)'
        actual='a,m,i'
        cases=['iall(a)==1','iany(array=a)==7','iparity(a)==0',
               'iall(array=a,mask=m)==1','iany(a,mask=m)==5','iparity(a,mask=m)==4',
               'iall(a,dim=1)==1','iany(a,dim=1,mask=m)==5',
               '.not.parity(mask=m)','parity(m(:3),dim=1).eqv..false.']
    elif group == 'norm':
        decl=f'real({kind}),allocatable::a(:),big(:),small(:)'
        power=30 if kind==4 else 250
        init=f'allocate(a(4),big(4),small(4))\na=[3.0_{kind},4.0_{kind},0.0_{kind},0.0_{kind}]\nbig=a*1.0e{power}_{kind}\nsmall=a*1.0e-{power}_{kind}'
        dummy=f'real({kind}),intent(in)::a(:),big(:),small(:)'
        actual='a,big,small,i'
        cases=[f'kind(norm2(a))=={kind}',f'abs(norm2(x=a)-5.0_{kind})<1e-5_{kind}',
               f'abs(norm2(a,dim=1)-5.0_{kind})<1e-5_{kind}',
               f'abs(norm2(big)/1e{power}_{kind}-5.0_{kind})<1e-5_{kind}',
               f'abs(norm2(small)/1e-{power}_{kind}-5.0_{kind})<1e-5_{kind}']
    else:
        decl=f'real({kind}),allocatable::a(:)\nlogical,allocatable::m(:)'
        init=f'allocate(a(-2:1),m(4))\na=[-3.0_{kind},7.0_{kind},-3.0_{kind},7.0_{kind}]\nm=[.false.,.true.,.true.,.true.]'
        dummy=f'real({kind}),intent(in)::a(:)\nlogical,intent(in)::m(:)'
        actual='a,m,i'
        cases=['sum(minloc(a))==1','sum(maxloc(a))==2',f'sum(findloc(a,-3.0_{kind}))==1','minloc(a,dim=1)==1','maxloc(array=a,dim=1)==2',
               'minloc(a,dim=1,back=.true.)==3','maxloc(a,dim=1,mask=m,back=.true.)==4',
               f'findloc(array=a,value=-3.0_{kind},dim=1,mask=m,kind=8)==3',
               f'findloc(a,99.0_{kind},dim=1)==0',
               f'findloc(a,-3.0_{kind},dim=1,back=modulo(i,2)==0)==merge(3,1,modulo(i,2)==0)']
    body='r=0\n'+'\n'.join(f'if(.not.({case})) r=r+1' for case in cases)+'\n'
    tail=''
    if context=='direct':
        loop=body+'y(i)=r';local=' local(r)'
    else:
        local=''
        params=actual
        if context=='function':
            loop=f'y(i)=probe({actual})'
            tail=f'contains\npure integer function probe({params}) result(r)\n{dummy}\ninteger,intent(in)::i\n{body}end function\n'
        else:
            loop=f'call probe({actual},y(i))'
            tail=f'contains\nsimple subroutine probe({params},r)\n{dummy}\ninteger,intent(in)::i\ninteger,intent(out)::r\n{body}end subroutine\n'
    return f"implicit none\ninteger::i,r,y(9000)\n{decl}\n{init}\ndo concurrent(i=1:9000){local}\n{loop}\nend do\nif(any(y/=0)) stop 1\nprint *, 'PASS'\n{tail}end\n"


class ParallelArrayIntrinsics(unittest.TestCase):
    def build_run(self,*args,**kwargs):
        previous=Path.cwd()
        with tempfile.TemporaryDirectory(prefix='rfort-array-intrinsic-modules-') as tmp:
            try:
                os.chdir(tmp)
                support.ParallelConcurrent.build_run(self,*args,**kwargs)
            finally:os.chdir(previous)

    def test_written_array_reduction_remains_serial(self):
        self.build_run("""implicit none
integer::i,a(9000)
a=1
do concurrent(i=1:9000)
a(i)=iany(a)
end do
if(any(a/=1)) stop 1
print *, 'PASS'
end
""",'PASS',reason='whole-array expressions require serial execution',threads=(4,))

    def test_standalone_example(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_array_intrinsics.f90').read_text(),'PASS')

    def test_norm_in_interpreter_and_native(self):
        test_deallocate.Deallocate.check(self,program('norm',kind=8).replace('9000','9'),'PASS',reference=False)

    def test_norm_evaluates_array_function_once(self):
        test_deallocate.Deallocate.check(self,"""implicit none
integer::calls=0
real(8)::answer
answer=norm2(values())
print *,answer,calls
contains
function values() result(a)
real(8)::a(2)
calls=calls+1
a=[3d0,4d0]
end function
end
""",'5.0 1',reference=False)

    def test_bits(self):
        for kind in (1,2,4,8):
            for context in ('direct','function','subroutine'):
                with self.subTest(kind=kind,context=context):
                    self.build_run(program('bits',context,kind),'PASS')

    def test_norm(self):
        for kind in (4,8):
            for context in ('direct','function','subroutine'):
                with self.subTest(kind=kind,context=context):
                    self.build_run(program('norm',context,kind),'PASS')

    def test_locations(self):
        for kind in (4,8):
            for context in ('direct','function','subroutine'):
                with self.subTest(kind=kind,context=context):
                    self.build_run(program('locations',context,kind),'PASS')

    def test_serial_and_unchecked(self):
        for group in ('bits','norm','locations'):
            self.build_run(program(group,'function'),'PASS',parallel=False,threads=(1,))
            self.build_run(program(group,'subroutine'),'PASS',checks=False)

    def test_multidimensional_runtime_dim(self):
        self.build_run("""implicit none
integer::i,y(9000)
real(8),allocatable::a(:,:)
integer,allocatable::b(:,:)
logical,allocatable::m(:,:)
allocate(a(2,2),b(2,2),m(2,2))
a=reshape([3d0,4d0,0d0,0d0],[2,2])
b=reshape([1,3,5,7],[2,2])
m=reshape([.true.,.false.,.true.,.false.],[2,2])
do concurrent(i=1:9000)
y(i)=probe(a,b,m,modulo(i,2)+1)
end do
if(any(y/=0)) stop 1
print *, 'PASS'
contains
pure integer function probe(a,b,m,d) result(r)
real(8),intent(in)::a(:,:)
integer,intent(in)::b(:,:),d
logical,intent(in)::m(:,:)
r=0
if(abs(sum(norm2(a,dim=d))-merge(5d0,7d0,d==1))>1d-12) r=r+1
if(sum(iall(b,dim=d))/=merge(6,4,d==1)) r=r+1
if(sum(iany(b,dim=d))/=merge(10,12,d==1)) r=r+1
if(sum(iparity(b,dim=d))/=merge(4,8,d==1)) r=r+1
if(count(parity(m,dim=d))/=merge(2,0,d==1)) r=r+1
if(sum(minloc(a,dim=d))/=merge(2,4,d==1)) r=r+1
if(sum(maxloc(a,dim=d))/=merge(3,2,d==1)) r=r+1
if(sum(findloc(a,0d0,dim=d))/=merge(1,4,d==1)) r=r+1
end function
end
""",'PASS')

    def test_empty_arrays_and_false_masks(self):
        self.build_run("""implicit none
integer::i,y(9000)
integer,allocatable::a(:)
real(8),allocatable::x(:)
logical,allocatable::m(:)
allocate(a(0),x(0),m(0))
do concurrent(i=1:9000)
y(i)=probe(a,x,m)
end do
if(any(y/=0)) stop 1
print *, 'PASS'
contains
pure integer function probe(a,x,m) result(r)
integer,intent(in)::a(:)
real(8),intent(in)::x(:)
logical,intent(in)::m(:)
r=0
if(iall(a)/=-1.or.iany(a)/=0.or.iparity(a)/=0) r=r+1
if(parity(m)) r=r+1
if(norm2(x)/=0d0) r=r+1
if(minloc(x,dim=1)/=0.or.maxloc(x,dim=1)/=0.or.findloc(x,0d0,dim=1)/=0) r=r+1
if(iall([1,2],mask=.false.)/=-1) r=r+1
if(iany([1,2],mask=.false.)/=0.or.iparity([1,2],mask=.false.)/=0) r=r+1
if(minloc([1,2],dim=1,mask=.false.)/=0) r=r+1
if(maxloc([1,2],dim=1,mask=.false.)/=0) r=r+1
if(findloc([1,2],2,dim=1,mask=.false.)/=0) r=r+1
end function
end
""",'PASS')

    def test_strided_input_and_integer_logical_searches(self):
        self.build_run("""implicit none
integer::i,y(9000),a(8)
logical::m(8)
a=[1,99,3,99,1,99,3,99]
m=[.true.,.false.,.false.,.false.,.true.,.false.,.false.,.false.]
do concurrent(i=1:9000)
y(i)=probe(a(1:8:2),m(1:8:2))
end do
if(any(y/=0)) stop 1
print *, 'PASS'
contains
pure integer function probe(a,m) result(r)
integer,intent(in)::a(:)
logical,intent(in)::m(:)
r=0
if(iall(a)/=1.or.iany(a)/=3.or.iparity(a)/=0.or.parity(m)) r=r+1
if(minloc(a,dim=1,back=.true.,kind=8)/=3) r=r+1
if(maxloc(a,dim=1,back=.true.)/=4) r=r+1
if(findloc(a,3,dim=1,back=.true.)/=4) r=r+1
if(findloc(m,.true.,dim=1,back=.true.)/=3) r=r+1
end function
end
""",'PASS')

    @unittest.skipUnless(shutil.which('gfortran'),'gfortran is not available')
    def test_gfortran_reference(self):
        for group in ('bits','norm','locations'):
            with self.subTest(group=group),tempfile.TemporaryDirectory() as tmp:
                path=Path(tmp)/'reference.f90';exe=Path(tmp)/'reference.exe'
                # Some gfortran builds underflow NORM2 on the extreme tiny case.
                # Compare moderate values here; the 3-4-5 identity checks the extremes independently.
                path.write_text(program(group).replace('do concurrent(i=1:9000) local(r)','do i=1,9000')
                                .replace('1.0e30_4','1.0e10_4').replace('1.0e-30_4','1.0e-10_4')
                                .replace('1e30_4','1e10_4').replace('1e-30_4','1e-10_4'))
                result=subprocess.run(['gfortran','-std=f2018','-fcheck=all',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                result=subprocess.run([str(exe)],capture_output=True,text=True,timeout=30)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(result.stdout.split(),['PASS'])


if __name__=='__main__':unittest.main()

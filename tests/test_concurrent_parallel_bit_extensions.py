"""Rotations, masks and unsigned comparisons in serial and parallel code."""
import os
from pathlib import Path
import subprocess
import shutil
import tempfile
import unittest
import test_concurrent_parallel as support
import test_deallocate
from test_record_read import COMPILER, ROOT


def program(kind, context):
    decl=f"integer({kind})::x,ones\ninteger::bits,p"
    body=f"""x=int(modulo(i,127),{kind})
ones=-1
bits=bit_size(x)
p=modulo(i,bits+1)
r=0
"""
    cases=[('ishftc(i=x,shift=bits)','x'),('ishftc(x,-bits)','x'),
      ('ishftc(ones,-1)','ones'),('ishftc(-huge(x)-int(1,kind(x)),1)', '1'),
      ('ishftc(size=bits,shift=0,i=x)','x'),
      ('ishftc(x,1,1)','x'),('ishftc(x,1,4)','ior(iand(x,not(int(15,kind(x)))),ior(iand(ishft(x,1),int(15,kind(x))),ibits(x,3,1)))'),
      (f'maskr(kind={kind},i=p)','ibits(ones,0,p)'),
      (f'maskl(p,kind={kind})','not(ibits(ones,0,bits-p))'),
      (f'kind(maskl(p,kind={kind}))',str(kind)),('kind(maskr(p))','4')]
    # Default-kind masks cannot accept counts exceeding 32.
    cases[-1]=('kind(maskr(int(modulo(p,33),8)))','4')
    for expression,expected in cases:
        body+=f'if ({expression}/={expected}) r=r+1\n'
    for expression in ['bge(i=ones,j=x)','bgt(ones,x)','ble(x,ones)','blt(j=ones,i=x)',
                       'bge(x,x)','ble(x,x)', '.not.bgt(x,x)', '.not.blt(x,x)']:
        body+=f'if (.not.({expression})) r=r+1\n'
    if context=='direct':
        header=f'integer::i,r,y(9000)\n{decl}';local=' local(x,ones,bits,p,r)';tail='';loop=body+'y(i)=r'
    elif context=='function':
        header='integer::i,y(9000)';local='';loop='y(i)=probe(i)'
        tail=f'contains\npure integer function probe(i) result(r)\ninteger,intent(in)::i\n{decl}\n{body}end function\n'
    else:
        header='integer::i,y(9000)';local='';loop='call probe(i,y(i))'
        tail=f'contains\nsimple subroutine probe(i,r)\ninteger,intent(in)::i\ninteger,intent(out)::r\n{decl}\n{body}end subroutine\n'
    return f"implicit none\n{header}\ndo concurrent(i=1:9000){local}\n{loop}\nend do\nif(any(y/=0)) stop 1\nprint *, 'PASS'\n{tail}end\n"


class BitExtensions(unittest.TestCase):
    build_run=support.ParallelConcurrent.build_run
    check=test_deallocate.Deallocate.check

    def test_fixture(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_bit_extensions.f90').read_text(),'PASS')

    def test_kinds_and_call_contexts(self):
        for kind in (1,2,4,8):
            for context in ('direct','function','subroutine'):
                with self.subTest(kind=kind,context=context):
                    self.build_run(program(kind,context),'PASS')

    @unittest.skipUnless(shutil.which('gfortran'),'gfortran is not available')
    def test_gfortran_reference(self):
        for kind in (1,2,4,8):
            with self.subTest(kind=kind),tempfile.TemporaryDirectory() as tmp:
                path=Path(tmp)/'reference.f90';exe=Path(tmp)/'reference.exe'
                # Gfortran does not yet accept LOCAL, so use an ordinary DO.
                source=program(kind,'direct').replace('do concurrent(i=1:9000) local(x,ones,bits,p,r)','do i=1,9000')
                path.write_text(source)
                result=subprocess.run(['gfortran','-std=f2018','-fcheck=all',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                result=subprocess.run([str(exe)],capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(result.stdout.split(),['PASS'])

    def test_serial_and_unchecked_parallel(self):
        source=program(8,'function')
        self.build_run(source,'PASS',parallel=False,threads=(1,))
        self.build_run(source,'PASS',checks=False)

    def test_mixed_kind_unsigned_comparisons(self):
        for left in (1,2,4,8):
            for right in (1,2,4,8):
                with self.subTest(left=left,right=right):
                    source=f"""implicit none
integer::i,y(9000)
integer({left})::a
integer({right})::b
do concurrent(i=1:9000) local(a,b)
a=-1
b=-1
y(i)=0
if (bge(a,b).neqv.{' .true.' if left>=right else '.false.'}) y(i)=y(i)+1
if (bgt(a,b).neqv.{' .true.' if left>right else '.false.'}) y(i)=y(i)+1
if (ble(a,b).neqv.{' .true.' if left<=right else '.false.'}) y(i)=y(i)+1
if (blt(a,b).neqv.{' .true.' if left<right else '.false.'}) y(i)=y(i)+1
end do
if(any(y/=0)) stop 1
print *, 'PASS'
end
"""
                    self.build_run(source,'PASS',opts=('2',))

    def test_constants_interpreter_and_arrays(self):
        self.check("""use iso_fortran_env,only:int8,int16,int32,int64
implicit none
integer(int8),parameter::a=maskl(8,kind=int8)
integer(int64),parameter::b=ishftc(1_int64,-64)
logical,parameter::c=blt(-1_int8,-1_int16)
integer::n=3,i
integer(int8)::x(3)
logical::z(3)
x=[1_int8,2_int8,3_int8]
print *, a,b,c,kind(popcnt(-1_int64))
print *,ishftc(x,1),maskr([0,1,8],kind=int8)
z=blt(x,-1_int16)
print *,z
end
""",'-1 1 T 4\n2 4 6 0 1 -1\nT T T\n')

    def test_runtime_array_operands(self):
        self.check("""implicit none
integer::n=3
integer(1),allocatable::x(:)
logical,allocatable::z(:)
integer(1),allocatable::rotated(:)
allocate(x(n),z(n))
x=[1_1,2_1,-1_1]
z=blt(x,-1_8)
print *,z
z=bgt(x,0_2)
print *,z
rotated=ishftc(x,1)
print *,rotated
end
""",'T T T\nT T T\n2 4 -1\n')

    def test_each_actual_evaluated_once(self):
        self.check("""implicit none
integer::calls=0,v
logical::b
v=ishftc(next(),1)
print *,v,calls
v=maskr(next())
print *,v,calls
b=blt(next(),100)
print *,b,calls
b=blt(0,next())
print *,b,calls
contains
integer function next()
calls=calls+1
next=calls
end function
end
""",'2 1\n3 2\nT 3\nT 4\n')

    def test_shadowed_names_stay_serial(self):
        self.build_run("""implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=maskr(i)
end do
if(any(y/=7)) stop 1
print *, 'PASS'
contains
integer function maskr(i)
integer,intent(in)::i
maskr=7
end function
end
""",'PASS',reason='serial fallback',threads=(4,))

    def test_logical_function_result(self):
        self.build_run("""implicit none
integer::i
logical::y(9000)
do concurrent(i=1:9000)
y(i)=ordered(int(-1,1),-1_8)
end do
if(.not.all(y)) stop 1
print *, 'PASS'
contains
pure logical function ordered(a,b)
integer(1),intent(in)::a
integer(8),intent(in)::b
ordered=blt(j=b,i=a)
end function
end
""",'PASS')

    def test_type_errors(self):
        for expression,message in [('maskr(1.0)','argument I must be INTEGER'),
                ('ishftc(1.0,1)','requires an INTEGER argument'),
                ('bge(1.0,1)','argument I must be INTEGER'),
                ('maskl(1,kind=16)','KIND must be 1, 2, 4 or 8')]:
            with self.subTest(expression=expression),tempfile.TemporaryDirectory() as tmp:
                path=Path(tmp)/'bad.f90';path.write_text(f'implicit none\nprint *,{expression}\nend\n')
                result=subprocess.run([str(COMPILER),str(path),'-o',str(Path(tmp)/'bad.exe')],capture_output=True,text=True,timeout=60)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(message,result.stderr)

    def test_invalid_controls(self):
        for expression in ('ishftc(1,1,0)','ishftc(1,1,33)','ishftc(1,5,4)',
                           'ishftc(1,-5,4)','maskl(-1)','maskr(9,kind=1)'):
            with self.subTest(expression=expression),tempfile.TemporaryDirectory() as tmp:
                path=Path(tmp)/'bad.f90';path.write_text(f'implicit none\nprint *,{expression}\nend\n')
                result=subprocess.run([str(COMPILER),str(path),'-o',str(Path(tmp)/'bad.exe')],capture_output=True,text=True,timeout=60)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(expression.split('(')[0].upper()+':',result.stderr)

    def test_runtime_errors_parallel(self):
        for expression in ('ishftc(i,1,n)','ishftc(i,n,4)','maskl(n,kind=1)','maskr(n,kind=1)'):
            for value,optimization in ((v,o) for v in (-9223372036854775807-1,65) for o in ('0','2')):
                with self.subTest(expression=expression,value=value,optimization=optimization),tempfile.TemporaryDirectory() as tmp:
                    path=Path(tmp)/'bad.f90';exe=Path(tmp)/'bad.exe'
                    path.write_text(f'implicit none\ninteger::i,y(9000)\ninteger(8)::n\nn={str(value)+"_8" if value>=0 else "(-huge(0_8)-1_8)"}\ndo concurrent(i=1:9000)\ny(i)={expression}\nend do\nprint *,y(1)\nend\n')
                    result=subprocess.run([str(COMPILER),'-O',optimization,'--parallel','--parallel-report','--nocheck',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=60)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertIn('native parallel worker emitted',result.stderr)
                    for threads in (1,2,4):
                        env=os.environ.copy();env['RFORT_NUM_THREADS']=str(threads)
                        result=subprocess.run([str(exe)],env=env,capture_output=True,text=True,timeout=15)
                        self.assertNotEqual(result.returncode,0)
                        self.assertIn(expression.split('(')[0].upper()+':',result.stderr)

if __name__=='__main__':
    unittest.main()

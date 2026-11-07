"""Presence specialization in PURE/SIMPLE calls from native parallel loops."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_concurrent_parallel_captures
from test_record_read import COMPILER, ROOT


class ParallelOptional(unittest.TestCase):
    check_source = test_concurrent_parallel_captures.ParallelCaptures.check_source

    def test_module_example(self):
        self.check_source((ROOT/'tests/programs/xconcurrent_parallel_optional.f90').read_text())

    def test_separate_module_compilation(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_optional.f90').read_text()
        module,main=source.split('program test_optional_parallel',1)
        with tempfile.TemporaryDirectory(prefix='rfort-optional-module-') as tmp:
            directory=Path(tmp)
            (directory/'score.f90').write_text(module)
            (directory/'main.f90').write_text('program test_optional_parallel'+main)
            for opt in ('0','2'):
                obj=directory/'score.obj'; exe=directory/'case.exe'
                result=subprocess.run([str(COMPILER),'-O',opt,'-J',tmp,'-c','score.f90','-o',str(obj)],cwd=tmp,capture_output=True,text=True,timeout=90)
                self.assertEqual(result.returncode,0,result.stderr)
                result=subprocess.run([str(COMPILER),'-O',opt,'--parallel','--parallel-report','-I',tmp,'main.f90',str(obj),'-o',str(exe)],cwd=tmp,capture_output=True,text=True,timeout=90)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertIn('native parallel worker emitted',result.stderr)
                self.assertNotIn('serial fallback',result.stderr)
                for count in (1,2,4):
                    result=subprocess.run([str(exe)],env=dict(os.environ,RFORT_NUM_THREADS=str(count),RFORT_PARALLEL_TRACE='1'),capture_output=True,text=True,timeout=30)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stdout.split(),['PASS'])
                    if count>1: self.assertIn(f'rfort parallel: {count} threads',result.stderr)

    def test_scalar_presence_keywords_and_expressions(self):
        self.check_source('''program p
implicit none
integer :: i,a(9001)
do concurrent(i=1:9001)
   a(i)=score(i)+score(i,shift=3)+score(i,2,4,.true.)
end do
do i=1,9001
   if(a(i)/=4*i+7) stop 1
end do
print *,"PASS"
contains
pure integer function score(i,scale,shift,enabled) result(r)
integer,intent(in)::i
integer,intent(in),optional::scale,shift
logical,intent(in),optional::enabled
r=i
if(present(scale)) r=r*scale
if(present(shift)) r=r+shift
if(present(enabled)) then
   if(.not.enabled) r=0
end if
end function
end program
''')

    def test_nested_forwarding_and_present_keyword(self):
        self.check_source('''program p
implicit none
integer::i,a(9001)
do concurrent(i=1:9001)
   a(i)=outer(i)+outer(i,5)
end do
do i=1,9001
   if(a(i)/=2*i+5) stop 1
end do
print *,"PASS"
contains
pure integer function leaf(i,offset) result(r)
integer,intent(in)::i
integer,intent(in),optional::offset
r=i
if(present(a=offset)) r=r+offset
end function
pure integer function outer(i,offset) result(r)
integer,intent(in)::i
integer,intent(in),optional::offset
r=leaf(i,offset)
end function
end program
''')

    def test_optional_arrays_fixed_allocatable_and_sections(self):
        self.check_source('''program p
implicit none
integer::i,a(9001),fixed(4)
integer,allocatable::values(:)
fixed=[1,2,3,4]
allocate(values(-2:3))
values=[2,4,6,8,10,12]
do concurrent(i=1:9001)
   a(i)=score(i)+score(i,fixed)+score(i,values)+score(i,values(-2:3:2))
end do
do i=1,9001
   if(a(i)/=4*i+70) stop 1
end do
print *,"PASS"
contains
pure integer function score(i,x) result(r)
integer,intent(in)::i
integer,intent(in),optional::x(:)
r=i
if(present(x)) r=r+sum(x)
end function
end program
''')

    def test_simple_subroutine_optional_inputs(self):
        self.check_source('''program p
implicit none
integer::i,a(9001),b(9001),x(3)
x=[1,2,3]
do concurrent(i=1:9001)
   call compute(i,y=a(i))
   call compute(i,offset=2,x=x,y=b(i))
end do
do i=1,9001
   if(a(i)/=i) stop 1
   if(b(i)/=i+8) stop 2
end do
print *,"PASS"
contains
simple subroutine compute(i,offset,x,y)
integer,intent(in)::i
integer,intent(in),optional::offset,x(:)
integer,intent(out)::y
y=i
if(present(offset)) y=y+offset
if(present(x)) y=y+sum(x)
end subroutine
end program
''')

    def test_present_does_not_read_undefined_value(self):
        self.check_source('''program p
implicit none
integer::i,a(9001),undefined
do concurrent(i=1:9001)
   a(i)=probe(undefined)+probe()
end do
if(any(a/=1)) stop 1
print *,"PASS"
contains
pure integer function probe(x) result(r)
integer,intent(in),optional::x
r=0
if(present(x)) r=1
end function
end program
''')

    def test_optional_output_keeps_serial_fallback(self):
        self.check_source('''program p
implicit none
integer::i,a(9001)
do concurrent(i=1:9001)
   call compute(i,a(i))
end do
do i=1,9001
   if(a(i)/=i) stop 1
end do
print *,"PASS"
contains
pure subroutine compute(i,x)
integer,intent(in)::i
integer,intent(out),optional::x
if(present(x)) x=i
end subroutine
end program
''',reason='serial fallback')

    def test_optional_numeric_kinds_and_logical(self):
        self.check_source('''program p
implicit none
integer::i
integer(kind=1)::k1
integer(kind=2)::k2
integer(kind=4)::k4
integer(kind=8)::k8
real::s
real(kind=8)::d,a(9001)
k1=1
k2=1
k4=1
k8=1
s=1
d=1
do concurrent(i=1:9001)
   a(i)=score(real(i,kind=8))+score(real(i,kind=8),s,d,k1,k2,k4,k8,.true.)
end do
do i=1,9001
   if(a(i)/=2d0*i+6d0) stop 1
end do
print *,"PASS"
contains
pure function score(x,s,d,k1,k2,k4,k8,enabled) result(r)
real(kind=8),intent(in)::x
real,intent(in),optional::s
real(kind=8),intent(in),optional::d
integer(kind=1),intent(in),optional::k1
integer(kind=2),intent(in),optional::k2
integer(kind=4),intent(in),optional::k4
integer(kind=8),intent(in),optional::k8
logical,intent(in),optional::enabled
real(kind=8)::r
r=x
if(present(s)) r=r+s
if(present(d)) r=r+d
if(present(k1)) r=r+k1
if(present(k2)) r=r+k2
if(present(k4)) r=r+k4
if(present(k8)) r=r+k8
if(present(enabled)) then
   if(.not.enabled) r=0
end if
end function
end program
''')

    def test_rank_two_real_and_logical_arrays(self):
        self.check_source('''program p
implicit none
integer::i
real(kind=8)::a(9001)
real(kind=8),allocatable::x(:,:)
logical,allocatable::flags(:,:)
allocate(x(-1:0,3:4),flags(2,2))
x=2d0
flags=.true.
do concurrent(i=1:9001)
   a(i)=score(i)+score(i,x,flags)
end do
do i=1,9001
   if(a(i)/=2d0*i+12d0) stop 1
end do
print *,"PASS"
contains
pure function score(i,x,flags) result(r)
integer,intent(in)::i
real(kind=8),intent(in),optional::x(:,:)
logical,intent(in),optional::flags(:,:)
real(kind=8)::r
r=real(i,kind=8)
if(present(x)) r=r+sum(x)
if(present(flags)) r=r+count(flags)
end function
end program
''')

    def test_runtime_guard_keeps_optional_scalar_reads_lazy(self):
        self.check_source('''program p
implicit none
integer::i,a(9001),b(9001),undefined
logical::enabled
enabled=.false.
if(enabled) undefined=3
do concurrent(i=1:9001)
   a(i)=probe(undefined,enabled)
   call query(undefined,enabled,b(i))
end do
if(any(a/=7).or.any(b/=7)) stop 1
print *,"PASS"
contains
pure integer function probe(x,enabled) result(r)
integer,intent(in),optional::x
logical,intent(in)::enabled
r=7
if(present(x)) then
   if(enabled) r=x
end if
end function
pure subroutine query(x,enabled,r)
integer,intent(in),optional::x
logical,intent(in)::enabled
integer,intent(out)::r
r=7
if(present(x)) then
   if(enabled) r=x
end if
end subroutine
end program
''')

    def test_optional_array_keeps_initialization_checks(self):
        self.check_source('''program p
implicit none
integer::i,a(9001),x(2)
x(1)=1
do concurrent(i=1:9001)
   a(i)=score(x)
end do
print *,"PASS"
contains
pure integer function score(x) result(r)
integer,intent(in),optional::x(:)
r=0
if(present(x)) r=sum(x)
end function
end program
''',runtime_error='uninitialized')

    def test_optional_array_keeps_bounds_checks(self):
        self.check_source('''program p
implicit none
integer::i,a(9001),x(2),index
x=1
index=3
do concurrent(i=1:9001)
   a(i)=score(x,index)
end do
print *,"PASS"
contains
pure integer function score(x,index) result(r)
integer,intent(in),optional::x(:)
integer,intent(in)::index
r=0
if(present(x)) r=x(index)
end function
end program
''',runtime_error='subscript outside')

    def test_optional_complex_input(self):
        self.check_source('''program p
implicit none
integer::i
complex(kind=8)::a(9001),offset
offset=cmplx(1d0,-2d0,kind=8)
do concurrent(i=1:9001)
   a(i)=score(i)+score(i,offset)
end do
do i=1,9001
   if(a(i)/=cmplx(2d0*i+1d0,-2d0,kind=8)) stop 1
end do
print *,"PASS"
contains
pure function score(i,offset) result(r)
integer,intent(in)::i
complex(kind=8),intent(in),optional::offset
complex(kind=8)::r
r=cmplx(real(i,kind=8),0d0,kind=8)
if(present(offset)) r=r+offset
end function
end program
''')

    def test_optional_value_retains_copy_semantics(self):
        self.check_source('''program p
implicit none
integer::i,a(9001),x
x=3
call compute(x,x)
if(x/=4) stop 1
do concurrent(i=1:9001)
   call compute(i,a(i))
end do
do i=1,9001
   if(a(i)/=i+1) stop 2
end do
print *,"PASS"
contains
pure subroutine compute(x,y)
integer,intent(in),optional,value::x
integer,intent(out)::y
y=9
if(present(x)) y=x+1
end subroutine
end program
''')

    def test_optional_input_may_alias_output_keeps_serial_fallback(self):
        self.check_source('''program p
implicit none
integer::i,a(9001)
a=1
do concurrent(i=1:9001)
   call compute(a(i),a(i))
end do
if(any(a/=1)) stop 1
print *,"PASS"
contains
pure subroutine compute(x,y)
integer,intent(in),optional::x
integer,intent(out)::y
y=1
if(present(x)) y=1
end subroutine
end program
''',reason='may alias')


if __name__ == '__main__':
    unittest.main()

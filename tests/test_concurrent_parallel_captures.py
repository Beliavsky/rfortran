"""Read-only shared state in native concurrent loops and PURE call chains."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER

class ParallelCaptures(unittest.TestCase):
    def check_source(self, source, reason=None, runtime_error=None):
        with tempfile.TemporaryDirectory(prefix='rfort-captures-') as tmp:
            directory=Path(tmp); path=directory/'case.f90'; path.write_text(source)
            for opt in ('0','2'):
                exe=directory/'case.exe'
                result=subprocess.run([str(COMPILER),'-O',opt,'--parallel','--parallel-report',str(path),'-o',str(exe)],cwd=tmp,capture_output=True,text=True,timeout=90)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertIn(reason or 'native parallel worker emitted',result.stderr)
                if reason is None: self.assertNotIn('serial fallback',result.stderr)
                for count in (1,2,4):
                    env=os.environ.copy(); env['RFORT_NUM_THREADS']=str(count);env['RFORT_PARALLEL_TRACE']='1'
                    run=subprocess.run([str(exe)],env=env,capture_output=True,text=True,timeout=30)
                    if runtime_error:
                        self.assertNotEqual(run.returncode,0)
                        self.assertIn(runtime_error,run.stderr)
                    else:
                        self.assertEqual(run.returncode,0,run.stderr)
                        self.assertEqual(run.stdout.split(),['PASS'])
                        if reason is None and count>1: self.assertIn(f'rfort parallel: {count} threads',run.stderr)
                        else: self.assertNotIn('rfort parallel:',run.stderr)

    def test_nested_module_calls_and_changes_between_loops(self):
        self.check_source('''module coefficients
implicit none
integer :: offset=2
integer :: a(3)=[2,3,4]
contains
pure integer function leaf(i) result(r)
integer,intent(in)::i
r=offset+a(2)+i
end function
pure integer function outer(i) result(r)
integer,intent(in)::i
r=leaf(i=i)
end function
end module
program p
use coefficients
implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=outer(i=i)
end do
do i=1,9000
if(y(i)/=i+5) stop 1
end do
offset=7
a(2)=8
do concurrent(i=1:9000)
y(i)=outer(i)
end do
do i=1,9000
if(y(i)/=i+15) stop 2
end do
print *, 'PASS'
end program
''')

    def test_direct_module_inputs_inside_procedure(self):
        self.check_source('''module data
implicit none
real :: coeff(2)=[2.,3.]
logical :: enabled=.true.
contains
subroutine solve()
integer :: i
real :: y(9000)
do concurrent(i=1:9000)
y(i)=coeff(1)*i
if(enabled) y(i)=y(i)+coeff(2)
end do
do i=1,9000
if(y(i)/=2*i+3) stop 1
end do
end subroutine
end module
program p
use data
implicit none
call solve()
print *, 'PASS'
end program
''')

    def test_pure_subroutine_reads_rank_two_module_data(self):
        self.check_source('''module data
implicit none
real :: coeff(-1:0,2:3)
logical :: enabled
contains
pure subroutine value(i,r)
integer,intent(in)::i
real,intent(out)::r
r=i
if(enabled) r=r+coeff(-1,2)
end subroutine
end module
program p
use data
implicit none
integer::i
real::y(9000)
coeff=4.0
enabled=.true.
do concurrent(i=1:9000)
call value(i,y(i))
end do
do i=1,9000
if(y(i)/=i+4) stop 1
end do
print *, 'PASS'
end program
''')

    def test_direct_runtime_module_array_and_reallocation(self):
        self.check_source('''module data
implicit none
real,allocatable :: coeff(:)
contains
subroutine solve()
integer :: i
real :: y(9000)
do concurrent(i=1:9000)
y(i)=coeff(i)
end do
if(any(y/=3.0)) stop 1
end subroutine
end module
program p
use data
implicit none
allocate(coeff(9000))
coeff=3.0
call solve()
deallocate(coeff)
allocate(coeff(-2:9001))
coeff=3.0
call solve()
print *, 'PASS'
end program
''')

    def test_pure_callee_reads_allocatable_module_array(self):
        self.check_source('''module data
implicit none
real,allocatable :: coeff(:)
contains
pure real function value(i) result(r)
integer,intent(in)::i
r=0.0
if(allocated(coeff)) r=coeff(i)
end function
end module
program p
use data
implicit none
integer::i
real::y(9000)
do concurrent(i=1:9000)
y(i)=value(i)
end do
if(any(y/=0.0)) stop 1
allocate(coeff(-2:9000))
coeff=3.0
do concurrent(i=1:9000)
y(i)=value(i)
end do
if(any(y/=3.0)) stop 2
deallocate(coeff)
allocate(coeff(9001))
coeff=4.0
do concurrent(i=1:9000)
y(i)=value(i)
end do
if(any(y/=4.0)) stop 3
print *, 'PASS'
end program
''')

    def test_promoted_host_inputs(self):
        self.check_source('''implicit none
integer :: offset,i,y(9000)
integer :: coeff(2)
offset=4
coeff=[2,3]
do concurrent(i=1:9000)
y(i)=value(i)
end do
do i=1,9000
if(y(i)/=i+7) stop 1
end do
print *, 'PASS'
contains
pure integer function value(i) result(r)
integer,intent(in)::i
r=i+offset+coeff(2)
end function
end
''')

    def test_module_output_keeps_serial_storage(self):
        self.check_source('''module data
implicit none
integer :: a(9000)
contains
subroutine solve()
integer :: i
do concurrent(i=1:9000)
a(i)=i
end do
end subroutine
end module
program p
use data
implicit none
integer::i
call solve()
do i=1,9000
if(a(i)/=i) stop 1
end do
print *, 'PASS'
end program
''',reason='writes to host or module storage')

    def test_writable_dummy_may_alias_module_input(self):
        self.check_source('''module data
implicit none
integer :: a(9000)
contains
subroutine solve(y)
integer,intent(out)::y(:)
integer::i
do concurrent(i=1:9000)
y(i)=a(i)+1
end do
end subroutine
end module
program p
use data
implicit none
integer::b(9000)
a=3
call solve(b)
if(any(b/=4)) stop 1
print *, 'PASS'
end program
''',reason='writable dummy may alias')

    def test_target_input_falls_back(self):
        self.check_source('''module data
implicit none
integer,target :: coeff=2
contains
pure integer function value(i) result(r)
integer,intent(in)::i
r=coeff+i
end function
end module
program p
use data
implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=value(i)
end do
if(y(9000)/=9002) stop 1
print *, 'PASS'
end program
''',reason='serial fallback')

    def test_guarded_capture_is_not_read_eagerly(self):
        self.check_source('''module data
implicit none
integer :: coeff
contains
pure integer function value(i) result(r)
integer,intent(in)::i
r=i
if(i<0) r=r+coeff
end function
end module
program p
use data
implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=value(i)
end do
if(y(9000)/=9000) stop 1
print *, 'PASS'
end program
''')

    def test_complex_module_callee_retains_serial_fallback(self):
        self.check_source('''module data
implicit none
integer,parameter :: dp=kind(0d0)
complex(kind=dp) :: coeff
contains
pure function value(i) result(r)
integer,intent(in)::i
complex(kind=dp) :: r
r=coeff*i
end function
end module
program p
use data
implicit none
integer::i
complex(kind=dp)::y(9000)
coeff=(2d0,3d0)
do concurrent(i=1:9000)
y(i)=value(i)
end do
if(abs(y(9000)-(18000d0,27000d0))>1d-8) stop 1
print *, 'PASS'
end program
''',reason='serial fallback')

    def test_named_kind_and_unrelated_character_constant(self):
        self.check_source('''module data
implicit none
integer,parameter :: dp=kind(0d0)
character(len=*),parameter :: label="unrelated"
real(kind=dp) :: coeff=2.0_dp
contains
pure real(kind=dp) function value(i) result(r)
integer,intent(in)::i
r=coeff*i
end function
end module
program p
use data
implicit none
integer::i
real(kind=dp)::y(9000)
do concurrent(i=1:9000)
y(i)=value(i)
end do
if(y(9000)/=18000.0_dp) stop 1
print *, 'PASS'
end program
''')

    def test_unproved_chain_keeps_original_lazy_reads(self):
        self.check_source('''module data
implicit none
integer :: coeff
contains
recursive pure integer function value(i) result(r)
integer,intent(in)::i
r=i
if(i<0) then
r=coeff+value(i+1)
end if
end function
end module
program p
use data
implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=value(i)
end do
if(y(9000)/=9000) stop 1
print *, 'PASS'
end program
''',reason='serial fallback')

    def test_private_data_with_use_only(self):
        self.check_source('''module data
implicit none
integer,private :: coeff=4
contains
pure integer function value(i)
integer,intent(in)::i
value=coeff+i
end function
end module
program p
use data,only:value
implicit none
integer::i,y(9000)
do concurrent(i=1:9000) default(none) shared(y)
y(i)=value(i)
end do
if(y(9000)/=9004) stop 1
print *, 'PASS'
end program
''')

    def test_separate_compilation_preserves_original_interface(self):
        with tempfile.TemporaryDirectory(prefix='rfort-capture-objects-') as tmp:
            directory=Path(tmp); module=directory/'data.f90'; main=directory/'main.f90'
            module.write_text('''module data
implicit none
integer,private :: coeff=4
contains
pure integer function value(i)
integer,intent(in)::i
value=coeff+i
end function
end module
''')
            main.write_text('''use data,only:value
implicit none
integer::i,y(9000)
if(value(1)/=5) stop 1
do concurrent(i=1:9000)
y(i)=value(i)
end do
if(y(9000)/=9004) stop 2
print *, 'PASS'
end
''')
            for opt in ('0','2'):
                obj=directory/'data.obj'; exe=directory/'case.exe'
                result=subprocess.run([str(COMPILER),'-O',opt,'-J',tmp,'-c',str(module),'-o',str(obj)],cwd=tmp,capture_output=True,text=True,timeout=90)
                self.assertEqual(result.returncode,0,result.stderr)
                result=subprocess.run([str(COMPILER),'-O',opt,'--parallel','--parallel-report','-I',tmp,str(main),str(obj),'-o',str(exe)],cwd=tmp,capture_output=True,text=True,timeout=90)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertIn('native parallel worker emitted',result.stderr)
                self.assertNotIn('serial fallback',result.stderr)
                for count in (1,2,4):
                    result=subprocess.run([str(exe)],env=dict(os.environ,RFORT_NUM_THREADS=str(count),RFORT_PARALLEL_TRACE='1'),capture_output=True,text=True,timeout=30)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stdout.split(),['PASS'])
                    if count>1: self.assertIn(f'rfort parallel: {count} threads',result.stderr)

    def test_captured_read_keeps_initialization_error(self):
        self.check_source('''module data
implicit none
integer :: coeff(2)
contains
pure integer function value(i) result(r)
integer,intent(in)::i
r=coeff(2)+i
end function
end module
program p
use data
implicit none
integer::i,y(9000)
coeff(1)=4
do concurrent(i=1:9000)
y(i)=value(i)
end do
print *, 'PASS'
end program
''',runtime_error='uninitialized variable `coeff`')

    def test_captured_read_keeps_bounds_error(self):
        self.check_source('''module data
implicit none
integer :: coeff(2)=[3,4]
contains
pure integer function value(i) result(r)
integer,intent(in)::i
r=coeff(i)
end function
end module
program p
use data
implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=value(i)
end do
print *, 'PASS'
end program
''',runtime_error='outside bounds')

if __name__=='__main__': unittest.main()

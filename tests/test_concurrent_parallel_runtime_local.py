"""Runtime-sized locality storage and pre-dispatch workspace budgets."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_concurrent_parallel
import test_concurrent_parallel_captures
from test_record_read import COMPILER, ROOT
from test_c_interop import C_COMPILER


class RuntimeLocalArrays(unittest.TestCase):
    check_source = test_concurrent_parallel_captures.ParallelCaptures.check_source
    build_run = test_concurrent_parallel.ParallelConcurrent.build_run

    def test_example(self):
        self.check_source((ROOT/'tests/programs/xconcurrent_parallel_runtime_local.f90').read_text())

    def test_local_repeated_calls_and_shapes(self):
        self.check_source('''program p
implicit none
integer::n,i,a(9001)
do n=1,4
   call compute(n,a)
   do i=1,size(a)
      if(a(i)/=n*i) stop 1
   end do
end do
print *,"PASS"
contains
subroutine compute(n,a)
integer,intent(in)::n
integer,intent(out)::a(:)
integer::i,work(n)
do concurrent(i=1:size(a)) local(work)
   work=i
   a(i)=sum(work)
end do
end subroutine
end program
''')

    def test_local_init_nondefault_bounds(self):
        self.check_source('''program p
implicit none
integer::n,i,a(9001)
do n=1,3
   call compute(n,a)
   do i=1,size(a)
      if(a(i)/=2*(n+1)+i) stop 1
   end do
end do
print *,"PASS"
contains
subroutine compute(n,a)
integer,intent(in)::n
integer,intent(out)::a(:)
integer::i,work(-n:0)
work=2
do concurrent(i=1:size(a)) local_init(work)
   work(-n)=work(-n)+i
   a(i)=sum(work)
end do
if(any(work/=2)) stop 2
end subroutine
end program
''')

    def test_empty_and_over_budget_dispatch_serially(self):
        source='''program p
implicit none
integer::i,a(9001)
call compute(SIZE,a)
do i=1,size(a)
   if(a(i)/=SIZE*i) stop 1
end do
print *,"PASS"
contains
subroutine compute(n,a)
integer,intent(in)::n
integer,intent(out)::a(:)
integer::i,work(n)
do concurrent(i=1:size(a)) local(work)
   work=i
   a(i)=sum(work)
end do
end subroutine
end program
'''
        for count in (0,4097):
            with self.subTest(count=count):
                self.build_run(source.replace('SIZE',str(count)),'PASS',dispatch=False)

    def test_multidimensional_complex_workspaces(self):
        self.check_source('''program p
implicit none
integer::n,i
complex(kind=8)::a(9001)
do n=1,3
   call compute(n,a)
   do i=1,size(a)
      if(a(i)/=cmplx(real(2*n*i+2*n+1,kind=8),real(6*n,kind=8),kind=8)) stop 1
   end do
end do
print *,"PASS"
contains
subroutine compute(n,a)
integer,intent(in)::n
complex(kind=8),intent(out)::a(:)
complex(kind=8)::work(n,2),initial(n,2)
integer::i
initial=cmplx(1d0,2d0,kind=8)
do concurrent(i=1:size(a)) local(work) local_init(initial)
   work=cmplx(real(i,kind=8),1d0,kind=8)
   initial(1,1)=initial(1,1)+1d0
   a(i)=sum(work)+sum(initial)
end do
do i=1,n
   if(initial(i,1)/=cmplx(1d0,2d0,kind=8)) stop 2
   if(initial(i,2)/=cmplx(1d0,2d0,kind=8)) stop 3
end do
end subroutine
end program
''')

    def test_per_iteration_initialization(self):
        self.check_source('''program p
implicit none
integer::a(9001)
call compute(2,a)
print *,"PASS"
contains
subroutine compute(n,a)
integer,intent(in)::n
integer,intent(out)::a(:)
integer::i,work(n)
work=99
do concurrent(i=1:size(a)) local(work)
   work(1)=i
   if(mod(i,2)==1) work(2)=1
   a(i)=sum(work)
end do
end subroutine
end program
''',runtime_error='uninitialized')

    def test_budget_sums_fixed_and_runtime_workspaces(self):
        self.build_run('''program p
implicit none
integer::a(9001),i
call compute(3000,a)
do i=1,size(a)
   if(a(i)/=i+1) stop 1
end do
print *,"PASS"
contains
subroutine compute(n,a)
integer,intent(in)::n
integer,intent(out)::a(:)
integer::i,work(n),fixed(2000)
do concurrent(i=1:size(a)) local(work,fixed)
   work(1)=i
   fixed(1)=1
   a(i)=work(1)+fixed(1)
end do
end subroutine
end program
''','PASS',dispatch=False)

    def test_established_shape_survives_bound_variable_change(self):
        self.check_source('''program p
implicit none
integer::n,i,a(9001)
n=2
call compute(n,a)
if(n/=3) stop 1
do i=1,size(a)
   if(a(i)/=2*i) stop 2
end do
print *,"PASS"
contains
subroutine compute(n,a)
integer,intent(inout)::n
integer,intent(out)::a(:)
integer::i,work(n)
n=n+1
do concurrent(i=1:size(a)) local(work)
   work=i
   a(i)=sum(work)
end do
end subroutine
end program
''')

    def test_allocation_failure_is_diagnosed(self):
        if not C_COMPILER:
            self.skipTest('a C compiler is required for allocator injection')
        source='''program p
implicit none
interface
   subroutine arm() bind(c)
   end subroutine
end interface
integer::a(9001)
call compute(2,a)
print *,"PASS"
contains
subroutine compute(n,a)
integer,intent(in)::n
integer,intent(out)::a(:)
integer::i,work(n)
call arm()
do concurrent(i=1:size(a)) local(work)
   work=i
   a(i)=sum(work)
end do
end subroutine
end program
'''
        shim='''typedef __SIZE_TYPE__ size_t;
extern void *calloc(size_t,size_t);
static int armed;
void arm(void) { armed=1; }
void *xalloc(size_t n,size_t w) {
    if(armed) { armed=0; return 0; }
    return calloc(n,w);
}
'''
        with tempfile.TemporaryDirectory(prefix='rfort-local-alloc-failure-') as tmp:
            directory=Path(tmp)
            (directory/'case.f90').write_text(source)
            (directory/'shim.c').write_text(shim)
            flags=['--target=x86_64-pc-windows-msvc'] if os.name=='nt' and 'clang' in Path(C_COMPILER).name else []
            def run(command):
                result=subprocess.run(list(map(str,command)),cwd=tmp,capture_output=True,text=True,timeout=90)
                self.assertEqual(result.returncode,0,result.stdout+result.stderr)
                return result
            run([C_COMPILER,*flags,'-c','shim.c','-o','shim.obj'])
            for opt in ('0','2'):
                result=run([COMPILER,'-O',opt,'--parallel','--parallel-report','-c','case.f90','-o','case.obj'])
                self.assertIn('native parallel worker emitted',result.stderr)
                obj=directory/'case.obj'
                code=obj.read_bytes()
                self.assertIn(b'calloc\0',code)
                obj.write_bytes(code.replace(b'calloc\0',b'xalloc\0'))
                run([COMPILER,'link','case.obj','shim.obj','-o','case.exe'])
                result=subprocess.run([str(directory/'case.exe')],cwd=tmp,capture_output=True,text=True,timeout=20)
                self.assertNotEqual(result.returncode,0)
                self.assertRegex(result.stderr.lower(),r'allocat.*(fail|memory)')

    def test_pure_subroutine_output_with_reduction(self):
        self.check_source('''program p
implicit none
integer::n,i,a(9001)
do n=1,3
   call compute(n,a)
   do i=1,size(a)
      if(a(i)/=n*i) stop 1
   end do
end do
print *,"PASS"
contains
subroutine compute(n,a)
integer,intent(in)::n
integer,intent(out)::a(:)
integer::i,work(n)
integer(kind=8)::total
work=99
total=7
do concurrent(i=1:size(a)) local(work) reduce(+:total)
   call fill(i,work)
   a(i)=sum(work)
   total=total+sum(work)
end do
if(total/=7_8+int(n,kind=8)*9001*9002/2) stop 2
if(any(work/=99)) stop 3
end subroutine
pure subroutine fill(i,work)
integer,intent(in)::i
integer,intent(out)::work(:)
work=i
end subroutine
end program
''')


if __name__ == '__main__':
    unittest.main()

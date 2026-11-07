"""Bounded LOCAL/LOCAL_INIT arrays in outlined native workers."""
import unittest
import test_concurrent_parallel
import test_concurrent_parallel_captures
from test_record_read import ROOT


class ParallelLocalArrays(unittest.TestCase):
    check_source = test_concurrent_parallel_captures.ParallelCaptures.check_source
    build_run = test_concurrent_parallel.ParallelConcurrent.build_run

    def test_example(self):
        self.check_source((ROOT/'tests/programs/xconcurrent_parallel_local_arrays.f90').read_text())

    def test_local_array_is_private_and_preserves_outer_object(self):
        source='''program p
implicit none
integer::i,t(2),a(9001)
t=99
do concurrent(i=1:9001) local(t)
   t(1)=i
   t(2)=2*i
   a(i)=sum(t)
end do
do i=1,9001
   if(a(i)/=3*i) stop 1
end do
if(any(t/=99)) stop 2
print *,"PASS"
end program
'''
        self.check_source(source)
        self.build_run(source,'PASS',checks=False)

    def test_local_init_restored_each_iteration_and_dispatch(self):
        self.check_source('''program p
implicit none
integer::i,t(-1:0),a(9001)
t=[2,3]
a=-1
do concurrent(i=1:9001,mod(i,2)==0) local_init(t)
   t(-1)=t(-1)+i
   a(i)=sum(t)
end do
do i=1,9001
   if(mod(i,2)==0) then
      if(a(i)/=5+i) stop 1
   else
      if(a(i)/=-1) stop 2
   end if
end do
if(any(t/=[2,3])) stop 3
t=[10,20]
do concurrent(i=1:9001) local_init(t)
   t(0)=t(0)+i
   a(i)=sum(t)
end do
do i=1,9001
   if(a(i)/=30+i) stop 4
end do
if(any(t/=[10,20])) stop 5
print *,"PASS"
end program
''')

    def test_local_initialization_resets_between_iterations(self):
        self.check_source('''program p
implicit none
integer::i,t(2),a(9001)
t=99
do concurrent(i=1:9001) local(t)
   t(1)=i
   if(mod(i,2)==1) t(2)=1
   a(i)=sum(t)
end do
print *,"PASS"
end program
''',runtime_error='uninitialized')

    def test_multidimensional_real_and_logical_arrays(self):
        self.check_source('''program p
implicit none
integer::i
real(kind=8)::t(-1:0,2),a(9001)
logical::flags(2)
t=2d0
do concurrent(i=1:9001) local_init(t) local(flags)
   t(-1,1)=t(-1,1)+real(i,kind=8)
   flags=[.true.,.false.]
   a(i)=sum(t)+count(flags)
end do
do i=1,9001
   if(a(i)/=9d0+i) stop 1
end do
if(any(t/=2d0)) stop 2
print *,"PASS"
end program
''')

    def test_complex_local_and_local_init_arrays(self):
        source='''program p
implicit none
integer::i
complex(kind=8)::t(2),scratch(2),a(9001)
t=cmplx(1d0,2d0,kind=8)
do concurrent(i=1:9001) local_init(t) local(scratch)
   t(1)=t(1)+cmplx(real(i,kind=8),0d0,kind=8)
   scratch(1)=t(1)
   scratch(2)=t(2)
   a(i)=sum(scratch)
end do
do i=1,9001
   if(a(i)/=cmplx(2d0+i,4d0,kind=8)) stop 1
end do
if(any(t/=cmplx(1d0,2d0,kind=8))) stop 2
print *,"PASS"
end program
'''
        for kind in (4,8):
            with self.subTest(kind=kind):
                self.check_source(source.replace('kind=8',f'kind={kind}'))

    def test_local_array_bounds_checks(self):
        self.check_source('''program p
implicit none
integer::i,t(2),a(9001),index
index=3
do concurrent(i=1:9001) local(t)
   t=1
   a(i)=t(index)
end do
print *,"PASS"
end program
''',runtime_error='subscript outside')

    def test_aggregate_workspace_budget_keeps_serial_fallback(self):
        self.check_source('''program p
implicit none
integer::i,t(2050),u(2050),a(9001)
do concurrent(i=1:9001) local(t,u)
   t(1)=i
   u(1)=1
   a(i)=t(1)+u(1)
end do
do i=1,9001
   if(a(i)/=i+1) stop 1
end do
print *,"PASS"
end program
''',reason='private-storage budget')

    def test_local_array_as_pure_subroutine_output(self):
        self.check_source('''program p
implicit none
integer::i,t(2),a(9001)
t=99
do concurrent(i=1:9001) local(t)
   call fill(i,t)
   a(i)=sum(t)
end do
do i=1,9001
   if(a(i)/=3*i) stop 1
end do
if(any(t/=99)) stop 2
print *,"PASS"
contains
pure subroutine fill(i,t)
integer,intent(in)::i
integer,intent(out)::t(:)
t(1)=i
t(2)=2*i
end subroutine
end program
''')

    def test_private_arrays_combined_with_reduction(self):
        self.check_source('''program p
implicit none
integer::i,t(2),u(2)
integer(kind=8)::total
u=[1,2]
total=7
do concurrent(i=1:9001) local(t) local_init(u) reduce(+:total)
   t=[i,2*i]
   u(1)=u(1)+i
   total=total+(sum(t)+sum(u))
end do
if(total/=7_8+4_8*9001*9002/2+3_8*9001) stop 1
if(any(u/=[1,2])) stop 2
print *,"PASS"
end program
''')

    def test_multiple_concurrent_indices(self):
        self.check_source('''program p
implicit none
integer::i,j,t(2),a(100,100)
do concurrent(i=1:100,j=1:100) local(t)
   t(1)=i
   t(2)=j
   a(i,j)=sum(t)
end do
do j=1,100
   do i=1,100
      if(a(i,j)/=i+j) stop 1
   end do
end do
print *,"PASS"
end program
''')

    def test_zero_sized_workspace_keeps_serial_fallback(self):
        self.check_source('''program p
implicit none
integer::i,t(0),a(9001)
do concurrent(i=1:9001) local(t)
   a(i)=size(t)
end do
if(any(a/=0)) stop 1
print *,"PASS"
end program
''',reason='zero-sized parallel LOCAL')

    def test_workspace_at_budget_boundary(self):
        self.check_source('''program p
implicit none
integer,parameter::capacity=4096
integer::i,t(capacity),a(9001)
do concurrent(i=1:9001) local(t)
   t(1)=i
   a(i)=t(1)
end do
do i=1,9001
   if(a(i)/=i) stop 1
end do
print *,"PASS"
end program
''')

    def test_complex_budget_counts_both_components(self):
        self.check_source('''program p
implicit none
integer::i
complex(kind=8)::t(2049)
real(kind=8)::a(9001)
do concurrent(i=1:9001) local(t)
   t(1)=cmplx(real(i,kind=8),0d0,kind=8)
   a(i)=real(t(1),kind=8)
end do
do i=1,9001
   if(a(i)/=real(i,kind=8)) stop 1
end do
print *,"PASS"
end program
''',reason='private-storage budget')


if __name__ == '__main__':
    unittest.main()

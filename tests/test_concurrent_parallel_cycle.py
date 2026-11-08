"""CYCLE of the concurrent iteration must preserve worker state and reductions."""
import unittest
from test_record_read import ROOT
import test_concurrent_parallel_captures
import test_concurrent_parallel
import test_fortran2023


class ConcurrentCycle(unittest.TestCase):
    check_source = test_concurrent_parallel_captures.ParallelCaptures.check_source

    def test_example(self):
        self.check_source((ROOT/'tests/programs/xconcurrent_parallel_cycle.f90').read_text())

    def test_nocheck(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_cycle.f90').read_text()
        test_concurrent_parallel.ParallelConcurrent.build_run(self, source, 'PASS', checks=False)

    def test_serial_modes(self):
        test_fortran2023.Fortran2023.check(self, '''implicit none
integer::i,s,t,a(9)
s=5
t=7
a=0
do concurrent(i=1:9) local_init(t) reduce(+:s)
 s=s+1
 if(mod(i,2)==0) then
  t=99
  cycle
 end if
 a(i)=t
end do
if(s/=14.or.t/=7.or.sum(a)/=35) stop 1
print *,"PASS"
end
''', ' PASS\n')

    def test_mask_and_signed_runtime_strides(self):
        for control in ('1:9001:2', '9001:1:-2', '9001:1:stride'):
            with self.subTest(control=control):
                self.check_source(f'''implicit none
integer::i,a(9001),stride
stride=-2
a=0
do concurrent(i={control},mod(i,3)/=0)
 if(mod(i,5)==0) cycle
 a(i)=i
end do
do i=1,9001
 if(mod(i,2)==1.and.mod(i,3)/=0.and.mod(i,5)/=0) then
  if(a(i)/=i) stop 1
 else
  if(a(i)/=0) stop 2
 end if
end do
print *,"PASS"
end
''')

    def test_multiple_indices(self):
        self.check_source('''implicit none
integer::i,j,a(101,101)
a=0
do concurrent(i=101:1:-1,j=1:101)
 if(i==j) cycle
 a(i,j)=i+j
end do
do j=1,101
 do i=1,101
  if(i==j) then
   if(a(i,j)/=0) stop 1
  else
   if(a(i,j)/=i+j) stop 2
  end if
 end do
end do
print *,"PASS"
end
''')

    def test_scalar_reductions_before_and_after_cycle(self):
        self.check_source('''implicit none
integer::i
integer(kind=8)::before,after
before=7
after=11
do concurrent(i=1:9001) reduce(+:before,after)
 before=before+1
 if(mod(i,2)==0) cycle
 after=after+1
end do
if(before/=9008.or.after/=4512) stop 1
print *,"PASS"
end
''')

    def test_all_iterations_cycle_with_array_reduction(self):
        self.check_source('''implicit none
integer::i,a(2)
a=[7,11]
do concurrent(i=1:9001) reduce(+:a)
 a=a+1
 cycle
 a=a+100
end do
if(any(a/=[9008,9012])) stop 1
print *,"PASS"
end
''')

    def test_runtime_local_init_and_inner_loop(self):
        self.check_source('''implicit none
call compute(3)
print *,"PASS"
contains
subroutine compute(n)
integer,intent(in)::n
integer::i,j,work(n),a(9001)
work=7
a=0
do concurrent(i=1:9001) local(j) local_init(work)
 if(mod(i,2)==0) then
  work=99
  cycle
 end if
 do j=1,n
  if(j==2) cycle
  work(j)=work(j)+i
 end do
 a(i)=sum(work)
end do
if(any(work/=7)) stop 1
do i=1,9001
 if(mod(i,2)==0) then
  if(a(i)/=0) stop 2
 else
  if(a(i)/=21+2*i) stop 3
 end if
end do
end subroutine
end
''')

    def test_cycle_does_not_initialize_local(self):
        self.check_source('''implicit none
integer::i,t,a(9001)
do concurrent(i=1:9001) local(t)
 if(mod(i,2)==1) then
  t=i
  a(i)=t
  cycle
 end if
 a(i)=t
end do
print *,"PASS"
end
''', runtime_error='uninitialized')

    def test_runtime_array_result_before_cycle(self):
        self.check_source('''implicit none
integer::i,n,t,a(9001)
n=3
do concurrent(i=1:9001) local(t)
 t=sum(values(n,i))
 a(i)=t
 if(mod(i,2)==0) cycle
 a(i)=a(i)+1
end do
do i=1,9001
 if(a(i)/=3*i+mod(i,2)) stop 1
end do
print *,"PASS"
contains
pure function values(n,k) result(r)
integer,intent(in)::n,k
integer::r(n)
r=k
end function
end
''')


if __name__=='__main__':
    unittest.main()

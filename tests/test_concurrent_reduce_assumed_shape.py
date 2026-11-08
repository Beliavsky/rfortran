"""Runtime descriptors for assumed-shape reductions and their temporaries."""
import unittest
from test_record_read import ROOT
import test_fortran2023
import test_concurrent_parallel
import test_concurrent_parallel_captures


class AssumedShapeReduction(unittest.TestCase):
    check = test_fortran2023.Fortran2023.check
    build_run = test_concurrent_parallel.ParallelConcurrent.build_run
    check_source = test_concurrent_parallel_captures.ParallelCaptures.check_source

    def check_all(self, source, dispatch=True):
        # Keep interpreter checks short; native parallel runs must exceed the
        # dispatch threshold to exercise workers rather than the serial path.
        self.check(source.replace('9001','3').replace('9011','13'), ' PASS\n')
        self.build_run(source, 'PASS', dispatch=dispatch)

    def test_minimal_example(self):
        self.check_all((ROOT/'tests/programs/xconcurrent_reduce_assumed_shape.f90').read_text(), dispatch=False)

    def test_nondefault_bounds_and_strided_actual(self):
        self.check_all('''implicit none
integer::x(-3:3),j
x=10
call accumulate(x(-3:3:2))
do j=-3,3
 if(mod(j+3,2)==0) then
  if(x(j)/=9011) stop 1
 else
  if(x(j)/=10) stop 2
 end if
end do
print *,"PASS"
contains
subroutine accumulate(a)
integer,intent(inout)::a(:)
integer::i
do concurrent(i=1:9001) reduce(+:a)
 a=a+1
end do
end subroutine
end
''')

    def test_rank_two_strided_section_and_nondefault_actual_bounds(self):
        self.check_all('''implicit none
integer::x(-1:3,2:7),j,k
x=10
call accumulate(x(-1:3:2,3:7:2))
do k=2,7
 do j=-1,3
  if(mod(j+1,2)==0.and.mod(k,2)==1) then
   if(x(j,k)/=9011) stop 1
  else
   if(x(j,k)/=10) stop 2
  end if
 end do
end do
print *,"PASS"
contains
subroutine accumulate(a)
integer,intent(inout)::a(:,:)
integer::i
do concurrent(i=1:9001) reduce(+:a)
 a=a+1
end do
end subroutine
end
''')

    def test_rank_three_minmax_and_logical(self):
        self.check_all('''implicit none
integer::a(2,1,2),b(2,1,2)
logical::tf(2,1,2)
a=100
b=-100
tf=.true.
call accumulate(a,b,tf)
if(any(a/=1).or.any(b/=9001).or.any(tf)) stop 1
print *,"PASS"
contains
subroutine accumulate(a,b,tf)
integer,intent(inout)::a(:,:,:),b(:,:,:)
logical,intent(inout)::tf(:,:,:)
integer::i
do concurrent(i=1:9001) reduce(min:a) reduce(max:b) reduce(.and.:tf)
 a=min(a,i)
 b=max(b,i)
 tf=tf.and.(i<9001)
end do
end subroutine
end
''')

    def test_empty_dummy_and_empty_concurrent_range(self):
        for shape, count in [('0',9001),('3',0)]:
            with self.subTest(shape=shape,count=count):
                self.check_all(f'''implicit none
integer::x({shape})
x=10
call accumulate(x)
if(any(x/=10)) stop 1
print *,"PASS"
contains
subroutine accumulate(a)
integer,intent(inout)::a(:)
integer::i
do concurrent(i=1:{count}) reduce(+:a)
 a=a+1
end do
end subroutine
end
''',dispatch=False)

    def test_real_kind_and_repeated_calls(self):
        self.check_all('''use iso_fortran_env,only:real64
implicit none
real(real64)::x(3)
x=10
call accumulate(x)
call accumulate(x)
if(any(x/=9011.0_real64)) stop 1
print *,"PASS"
contains
subroutine accumulate(a)
real(real64),intent(inout)::a(:)
integer::i
do concurrent(i=1:9001) reduce(+:a)
 a=a+0.5_real64
end do
end subroutine
end
''')

    def test_uninitialized_input_is_still_diagnosed(self):
        self.check_source('''implicit none
integer::x(2),n
call random_seed(size=n)
if(n>0) x(1)=10
if(n<0) x(2)=10
call accumulate(x)
print *,"PASS"
contains
subroutine accumulate(a)
integer,intent(inout)::a(:)
integer::i
do concurrent(i=1:9001) reduce(+:a)
 a=a+1
end do
end subroutine
end
''',runtime_error='uninitialized')

    def test_heap_temporaries_in_serial_control_flow(self):
        self.check('''implicit none
integer,allocatable::x(:)
integer::n
n=3
allocate(x(n))
x=10
call update(x,n)
if(any(x/=16)) stop 1
print *,"PASS"
contains
subroutine update(a,n)
integer,intent(in)::n
integer,intent(inout)::a(:)
integer::i
do i=1,3
 select case(i)
 case(1)
  a=a+1
 case(2)
  if(n>0) a=a+2
 case default
  a=a+3
 end select
end do
end subroutine
end
''',' PASS\n')


if __name__=='__main__':
    unittest.main()

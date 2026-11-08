"""Disjoint rows/columns/slabs with variable subscripts inside each owner."""
import unittest
from test_record_read import ROOT
import test_concurrent_parallel_captures
import test_concurrent_parallel
import test_fortran2023


class ParallelSlabs(unittest.TestCase):
    check_source=test_concurrent_parallel_captures.ParallelCaptures.check_source

    def test_example(self):
        self.check_source((ROOT/'tests/programs/xconcurrent_parallel_slabs.f90').read_text())

    def test_rows_and_columns(self):
        for axes in ('j,i','i,j'):
            with self.subTest(axes=axes):
                dims='3,9001' if axes=='j,i' else '9001,3'
                self.check_source(f'''implicit none
integer::i,j,a({dims}),b({dims})
real::scale(9001)
scale=2
b=3
do concurrent(i=1:9001) local(j)
 do j=1,3
  a({axes})=int(b({axes})*scale(i))+j
 end do
end do
do i=1,9001
 do j=1,3
  if(a({axes})/=6+j) stop 1
 end do
end do
print *,"PASS"
end
''')

    def test_negative_stride_and_explicit_bounds(self):
        self.check_source('''implicit none
integer::i,j,a(-1:1,-2:8998),step
step=-1
do concurrent(i=8998:-2:step) local(j)
 do j=1,-1,step
  a(j,i)=i+j
 end do
end do
do i=-2,8998
 do j=-1,1
  if(a(j,i)/=i+j) stop 1
 end do
end do
print *,"PASS"
end
''')

    def test_multiple_controls_and_nested_inner_loops(self):
        self.check_source('''implicit none
integer::i,j,k,l,a(2,101,2,101)
do concurrent(i=1:101,j=101:1:-1) local(k,l)
 do k=1,2
  do l=1,2
   a(k,j,l,i)=i+j+k+l
  end do
 end do
end do
do i=1,101
 do j=1,101
  do k=1,2
   do l=1,2
    if(a(k,j,l,i)/=i+j+k+l) stop 1
   end do
  end do
 end do
end do
print *,"PASS"
end
''')

    def test_while_and_branch_transfers(self):
        self.check_source('''implicit none
integer::i,j,a(3,9001)
a=0
do concurrent(i=1:9001,mod(i,3)/=0) local(j)
 j=0
 do while(j<3)
  j=j+1
  select case(j)
  case(2)
   cycle
  case default
   a(j,i)=j+i
  end select
 end do
end do
do i=1,9001
 do j=1,3
  if(mod(i,3)/=0.and.j/=2) then
   if(a(j,i)/=j+i) stop 1
  else
   if(a(j,i)/=0) stop 2
  end if
 end do
end do
print *,"PASS"
end
''')

    def test_invariant_offset_and_private_subscript_expression(self):
        self.check_source('''implicit none
integer::i,j,a(6,9001),offset
offset=1
a=0
do concurrent(i=0:9000) local(j)
 do j=1,3
  a(2*j,i+offset)=i+j
 end do
end do
do i=0,9000
 do j=1,3
  if(a(2*j,i+1)/=i+j) stop 1
  if(a(2*j-1,i+1)/=0) stop 2
 end do
end do
print *,"PASS"
end
''')

    def test_allocatable_and_assumed_shape_dummy(self):
        self.check_source('''implicit none
integer,allocatable::a(:,:)
integer::i,j,n
n=9001
allocate(a(-1:1,-2:n-3))
call fill(a)
do i=-2,n-3
 do j=-1,1
  if(a(j,i)/=10*(i+3)+j+2) stop 1
 end do
end do
print *,"PASS"
contains
subroutine fill(x)
integer,intent(out)::x(:,:)
integer::i,j
do concurrent(i=1:size(x,2)) local(j)
 do j=1,size(x,1)
  x(j,i)=10*i+j
 end do
end do
end subroutine
end
''')

    def test_pure_output_call_and_section_reads(self):
        self.check_source('''implicit none
integer::i,j,a(3,9001),s(9001)
do concurrent(i=1:9001) local(j)
 do j=1,3
  call value(i,j,a(j,i))
 end do
 call value(i,0,a(1,i))
 s(i)=sum(a(:,i))
end do
do i=1,9001
 if(s(i)/=3*i+5) stop 1
end do
print *,"PASS"
contains
pure subroutine value(i,j,r)
integer,intent(in)::i,j
integer,intent(out)::r
r=i+j
end subroutine
end
''')

    def test_runtime_allocatable_shape_inquiries(self):
        self.check_source('''implicit none
integer,allocatable::a(:,:)
integer::i,j,n
n=9001
allocate(a(-1:1,-2:n-3))
do concurrent(i=lbound(a,2):ubound(a,2)) local(j)
 do j=lbound(array=a,dim=1),ubound(array=a,dim=1)
  a(j,i)=10*i+j
 end do
end do
do i=-2,n-3
 do j=-1,1
  if(a(j,i)/=10*i+j) stop 1
 end do
end do
print *,"PASS"
end
''')

    def test_rank_fifteen(self):
        shape=','.join(['2']+['1']*13+['9001'])
        indices=','.join(['j']+['1']*13+['i'])
        self.check_source(f'''implicit none
integer::i,j,a({shape})
do concurrent(i=1:9001) local(j)
 do j=1,2
  a({indices})=i+j
 end do
end do
do i=1,9001
 do j=1,2
  if(a({indices})/=i+j) stop 1
 end do
end do
print *,"PASS"
end
''')

    def test_section_output_with_private_bounds(self):
        self.check_source('''implicit none
integer::i,j,a(4,9001)
do concurrent(i=1:9001) local(j)
 do j=1,3,2
  call fill(i,a(j:j+1,i))
 end do
end do
if(any(a(:,1)/=1).or.any(a(:,9001)/=9001)) stop 1
print *,"PASS"
contains
pure subroutine fill(i,x)
integer,intent(in)::i
integer,intent(out)::x(:)
x=i
end subroutine
end
''')

    def test_intrinsic_element_types(self):
        for declaration,value,comparison in [
            ('integer(kind=8)','int(i,kind=8)+j','a(j,i)/=int(i,kind=8)+j'),
            ('real(kind=8)','real(i,kind=8)+j','abs(a(j,i)-i-j)>1.0d-10'),
            ('logical','mod(i+j,2)==0','a(j,i).neqv.(mod(i+j,2)==0)'),
            ('complex(kind=8)','cmplx(i,j,kind=8)','abs(a(j,i)-cmplx(i,j,kind=8))>1.0d-10'),
        ]:
            with self.subTest(declaration=declaration):
                self.check_source(f'''implicit none
integer::i,j
{declaration}::a(2,9001)
do concurrent(i=1:9001) local(j)
 do j=1,2
  a(j,i)={value}
 end do
end do
do i=1,9001
 do j=1,2
  if({comparison}) stop 1
 end do
end do
print *,"PASS"
end
''')

    def test_reads_do_not_cross_owned_column(self):
        self.check_source('''implicit none
integer::i,j,a(3,9002)
a=1
do concurrent(i=2:9001) local(j)
 do j=1,3
  a(j,i)=a(j,i-1)
 end do
end do
print *,"PASS"
end
''',reason='not confined to the current iteration')

    def test_ownership_cannot_shift_or_change_axis(self):
        for second in ('a(1,i+1)=2','a(i,1)=2'):
            with self.subTest(second=second):
                self.check_source(f'''implicit none
integer::i,j,a(4,4)
a=0
do concurrent(i=1:3) local(j)
 do j=1,3
  a(j,i)=1
 end do
 {second}
end do
print *,"PASS"
end
''',reason='same index tuple')

    def test_private_offset_cannot_define_owner(self):
        self.check_source('''implicit none
integer::i,j,k,a(3,9002)
do concurrent(i=1:9001) local(j,k)
 k=1
 do j=1,3
  a(j,i+k)=i
 end do
end do
print *,"PASS"
end
''',reason='invariant subscripts or index offsets')

    def test_missing_concurrent_control_falls_back(self):
        self.check_source('''implicit none
integer::i,j,k,a(2,101)
do concurrent(i=1:101,j=1:101) local(k)
 do k=1,2
  a(k,i)=i
 end do
end do
print *,"PASS"
end
''',reason='every concurrent index exactly once')

    def test_vector_offset_cannot_define_owner(self):
        for owner in ('i+offsets(:)','i+[0,1]'):
            with self.subTest(owner=owner):
                self.check_source(f'''implicit none
integer::i,j,a(9002,2),offsets(2)
offsets=[0,1]
a=0
do concurrent(i=1:9001) local(j)
 do j=1,2
  a({owner},j)=i
 end do
end do
print *,"PASS"
end
''',reason='invariant subscripts or index offsets')

    def test_bounds_and_initialization_checks(self):
        for body,error in [('a(j,i)=j','outside'),('a(1,i)=a(2,i)','uninitialized')]:
            with self.subTest(error=error):
                self.check_source(f'''implicit none
integer::i,j,a(3,9001)
do concurrent(i=1:9001) local(j)
 do j=1,4
  {body}
 end do
end do
print *,"PASS"
end
''',runtime_error=error)

    def test_nocheck_and_serial_modes(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_slabs.f90').read_text()
        test_concurrent_parallel.ParallelConcurrent.build_run(self,source,'PASS',checks=False)
        test_fortran2023.Fortran2023.check(self,source.replace('9001','9'),' PASS\n')


if __name__=='__main__':
    unittest.main()

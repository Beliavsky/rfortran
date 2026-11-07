"""Optional writable arguments retain parallel output and initialization proofs."""
import unittest
import test_concurrent_parallel_captures
from test_record_read import ROOT


class ParallelOptionalOutputs(unittest.TestCase):
    check_source = test_concurrent_parallel_captures.ParallelCaptures.check_source

    def test_module_example(self):
        self.check_source((ROOT/'tests/programs/xconcurrent_parallel_optional_outputs.f90').read_text())

    def test_scalars_omitted_keywords_and_forwarding(self):
        self.check_source('''program p
implicit none
integer::i,a(9001),b(9001),c(9001)
b=10
do concurrent(i=1:9001)
   call outer(i,y=a(i))
   call outer(i,z=b(i),y=c(i))
   call outer(i,z=b(i))
   call outer(i)
end do
do i=1,9001
   if(a(i)/=i.or.b(i)/=10+2*i.or.c(i)/=i) stop 1
end do
print *,"PASS"
contains
pure subroutine leaf(i,z,y)
integer,intent(in)::i
integer,intent(inout),optional::z
integer,intent(out),optional::y
if(present(z)) z=z+i
if(present(y)) y=i
end subroutine
pure subroutine outer(i,z,y)
integer,intent(in)::i
integer,intent(inout),optional::z
integer,intent(out),optional::y
call leaf(i,z,y)
end subroutine
end program
''')

    def test_runtime_sized_multidimensional_sections(self):
        self.check_source('''program p
implicit none
integer::i
real(kind=8),allocatable::a(:,:,:),b(:,:,:)
allocate(a(2,3,9001),b(2,3,9001))
b=1d0
do concurrent(i=1:9001)
   call fill(i,a(:,:,i),b(:,:,i))
   call fill(i,updated=b(:,:,i))
end do
do i=1,9001
   if(any(a(:,:,i)/=real(i,kind=8))) stop 1
   if(any(b(:,:,i)/=3d0)) stop 2
end do
print *,"PASS"
contains
simple subroutine fill(i,result,updated)
integer,intent(in)::i
real(kind=8),intent(out),optional::result(:,:)
real(kind=8),intent(inout),optional::updated(:,:)
if(present(result)) result=real(i,kind=8)
if(present(updated)) updated=updated+1d0
end subroutine
end program
''')

    def test_numeric_and_logical_scalar_outputs(self):
        self.check_source('''program p
implicit none
integer::i,a(9001)
real(kind=8)::b(9001)
logical::flags(9001)
complex(kind=8)::c(9001)
do concurrent(i=1:9001)
   call make(i,a(i),b(i),flags(i),c(i))
   call make(i,flag=flags(i))
end do
do i=1,9001
   if(a(i)/=i.or.b(i)/=real(i,kind=8)) stop 1
   if(flags(i).neqv.(mod(i,2)==0)) stop 2
   if(c(i)/=cmplx(real(i,kind=8),-real(i,kind=8),kind=8)) stop 3
end do
print *,"PASS"
contains
pure subroutine make(i,n,r,flag,z)
integer,intent(in)::i
integer,intent(out),optional::n
real(kind=8),intent(out),optional::r
logical,intent(out),optional::flag
complex(kind=8),intent(out),optional::z
if(present(n)) n=i
if(present(r)) r=real(i,kind=8)
if(present(flag)) flag=mod(i,2)==0
if(present(z)) z=cmplx(real(i,kind=8),-real(i,kind=8),kind=8)
end subroutine
end program
''')

    def test_inout_requires_initialized_elements(self):
        self.check_source('''program p
implicit none
integer::i,a(9001)
a(1:9000)=1
do concurrent(i=1:9001)
   call update(a(i))
end do
print *,"PASS"
contains
pure subroutine update(x)
integer,intent(inout),optional::x
if(present(x)) x=x+1
end subroutine
end program
''',runtime_error='uninitialized')

    def test_optional_output_keeps_bounds_checks(self):
        self.check_source('''program p
implicit none
integer::i,a(9001),offset
offset=1
do concurrent(i=1:9001)
   call fill(i,a(i+offset))
end do
print *,"PASS"
contains
pure subroutine fill(i,x)
integer,intent(in)::i
integer,intent(out),optional::x
if(present(x)) x=i
end subroutine
end program
''',runtime_error='subscript outside')

    def test_out_undefines_previous_value(self):
        self.check_source('''program p
implicit none
integer::i,a(9001)
a=1
do concurrent(i=1:9001)
   call fill(i,a(i))
end do
print *,a(9001)
print *,"PASS"
contains
pure subroutine fill(i,x)
integer,intent(in)::i
integer,intent(out),optional::x
if(present(x)) then
   if(i<9001) x=i
end if
end subroutine
end program
''',runtime_error='uninitialized')

    def test_out_section_undefines_unwritten_elements(self):
        self.check_source('''program p
implicit none
integer::i,a(2,9001)
a=1
do concurrent(i=1:9001)
   call fill(i,a(:,i))
end do
print *,a(2,9001)
print *,"PASS"
contains
pure subroutine fill(i,x)
integer,intent(in)::i
integer,intent(out),optional::x(:)
if(present(x)) x(1)=i
end subroutine
end program
''',runtime_error='uninitialized')

    def test_same_base_output_arguments_keep_alias_fallback(self):
        self.check_source('''program p
implicit none
integer::i,a(2,9001)
do concurrent(i=1:9001)
   call fill(i,a(1,i),a(2,i))
end do
do i=1,9001
   if(a(1,i)/=i.or.a(2,i)/=-i) stop 1
end do
print *,"PASS"
contains
pure subroutine fill(i,x,y)
integer,intent(in)::i
integer,intent(out),optional::x,y
if(present(x)) x=i
if(present(y)) y=-i
end subroutine
end program
''',reason='may alias')

    def test_unproved_disjoint_sections_keep_serial_fallback(self):
        self.check_source('''program p
implicit none
integer::i,a(18002)
do concurrent(i=1:9001)
   call fill(a(2*i-1:2*i))
end do
if(any(a/=1)) stop 1
print *,"PASS"
contains
pure subroutine fill(x)
integer,intent(out),optional::x(:)
if(present(x)) x=1
end subroutine
end program
''',reason='serial fallback')


if __name__ == '__main__':
    unittest.main()

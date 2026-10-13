"""Heap descriptor association for numeric ALLOCATABLE subroutine dummies."""
import unittest
import test_deallocate
from test_record_read import ROOT

class AllocatableDummies(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_matrix_reproducer(self):
        self.check((ROOT/'tests/176_allocatable_dummies.f90').read_text(encoding='utf-8'),
                   'T 0 3 0\nT 2 2 4\nT 3 2 6\n')

    def test_runtime_output_and_forwarding(self):
        self.check("""program main
integer::n
real(8),allocatable::a(:,:)
read (*,*) n
call outer(a,n)
print *,shape(a),sum(a)
call outer(a,n+1)
print *,shape(a),sum(a)
deallocate(a)
contains
subroutine outer(b,n)
real(8),allocatable,intent(out)::b(:,:)
integer,intent(in)::n
call inner(b,n)
end subroutine outer
subroutine inner(c,n)
real(8),allocatable,intent(out)::c(:,:)
integer,intent(in)::n
allocate(c(n,2))
c=2.5d0
end subroutine inner
end program main
""",'3 2 15\n4 2 20\n',data='3\n')

    def test_unallocated_input_and_inout_reallocation(self):
        self.check("""program main
integer,allocatable::a(:,:)
call show(a)
call change(a,3)
call show(a)
call change(a,2)
call show(a)
deallocate(a)
contains
subroutine show(b)
integer,allocatable,intent(in)::b(:,:)
print *,allocated(b),kind(b)
if(allocated(b)) print *,lbound(b),ubound(b),sum(b)
end subroutine show
subroutine change(b,n)
integer,allocatable,intent(inout)::b(:,:)
integer,intent(in)::n
if(allocated(b)) deallocate(b)
allocate(b(-1:n,2:3))
b=4
end subroutine change
end program main
""",'F 4\nT 4\n-1 2 3 3 40\nT 4\n-1 2 2 3 32\n')

    def test_intent_out_entry_and_return_without_allocation(self):
        self.check("""program main
integer,allocatable::a(:)
allocate(a(2))
a=7
call clear(a)
print *,allocated(a)
call clear(a)
print *,allocated(a)
contains
subroutine clear(b)
integer,allocatable,intent(out)::b(:)
print *,allocated(b)
end subroutine clear
end program main
""",'F\nF\nF\nF\n')

    def test_rank_three_and_rank_fifteen(self):
        for rank in (3,15):
            shape=':'+ ',:'*(rank-1)
            extents='n'+',1'*(rank-1)
            self.check('program main\ninteger::n\nlogical,allocatable::a('+shape+')\n'
                       'read (*,*) n\ncall make(a,n)\nprint *,size(a),all(a)\n'
                       'deallocate(a)\ncontains\nsubroutine make(b,n)\n'
                       'logical,allocatable,intent(out)::b('+shape+')\n'
                       'integer,intent(in)::n\nallocate(b('+extents+'))\nb=.true.\n'
                       'end subroutine make\nend program main\n','3 T\n',data='3\n')

    def test_callee_local_shadows_actual_name(self):
        self.check("""program main
integer,allocatable::a(:,:)
call make(a)
print *,shape(a),sum(a)
deallocate(a)
contains
subroutine make(b)
integer,allocatable,intent(out)::b(:,:)
integer,allocatable::a(:,:)
integer::n
n=3
allocate(a(n,1),b(2,2))
a=7
b=2
print *,shape(a),sum(a)
deallocate(a)
end subroutine make
end program main
""",'3 1 21\n2 2 8\n')

    def test_output_resets_element_initialization(self):
        self.check("""program main
integer::n
integer,allocatable::a(:,:)
read (*,*) n
allocate(a(n,2))
a=7
call partial(a,n)
print *,a(n,2)
contains
subroutine partial(b,n)
integer,allocatable,intent(out)::b(:,:)
integer,intent(in)::n
allocate(b(n,2))
b(1,1)=1
end subroutine partial
end program main
""",error='uninitialized',data='3\n')

    def test_same_dummy_name_optional_and_unallocated_forwarding(self):
        self.check("""program main
integer,allocatable::a(:,:)
call outer(a)
call outer(n=3,a=a)
print *,shape(a),sum(a)
deallocate(a)
contains
subroutine outer(a,n)
integer,allocatable,intent(inout)::a(:,:)
integer,intent(in),optional::n
if(present(n)) then
call inner(a,n)
else
print *,allocated(a)
end if
end subroutine outer
subroutine inner(a,n)
integer,allocatable,intent(inout)::a(:,:)
integer,intent(in)::n
allocate(a(n,2))
a=3
end subroutine inner
end program main
""",'F\n3 2 18\n')

    def test_module_actual_and_output_larger_than_input(self):
        self.check("""module m
integer,allocatable::a(:,:)
contains
subroutine make(b,v)
integer,allocatable,intent(out)::b(:,:)
integer,intent(in)::v(:)
allocate(b(size(v)+2,2))
b=4
end subroutine make
end module m
use m
implicit none
call make(a,[1,2,3])
print *,shape(a),sum(a)
deallocate(a)
end
""",'5 2 40\n')

    def test_input_cannot_be_forwarded_as_writable(self):
        self.reject("""program main
integer,allocatable::a(:,:)
call outer(a)
contains
subroutine outer(b)
integer,allocatable,intent(in)::b(:,:)
call inner(b)
end subroutine outer
subroutine inner(c)
integer,allocatable,intent(out)::c(:,:)
allocate(c(2,2))
end subroutine inner
end program main
""",'INTENT(IN)')

    def test_default_integer_matches_int32(self):
        self.check("""program main
use iso_fortran_env,only:int32
implicit none
integer(int32),allocatable::a(:,:)
call make(a)
print *,kind(a),sum(a)
deallocate(a)
contains
subroutine make(b)
integer,allocatable,intent(out)::b(:,:)
allocate(b(2,2))
b=3
end subroutine make
end program main
""",'4 12\n')

    def test_copy_from_fixed_matrix(self):
        for rows in (0,2):
            self.check(f"""program main
integer::a({rows},3)
integer,allocatable::b(:,:)
a=4
call copy(a,b)
print *,shape(b),sum(b)
deallocate(b)
contains
subroutine copy(x,y)
integer,intent(in)::x(:,:)
integer,allocatable,intent(out)::y(:,:)
allocate(y(size(x,1),size(x,2)))
y=x
end subroutine copy
end program main
""",f'{rows} 3 {rows*12}\n')

    def test_invalid_actual_and_input_mutation(self):
        self.reject("""program main
integer::a(2)
a=1
call show(a)
contains
subroutine show(b)
integer,allocatable,intent(in)::b(:)
print *,allocated(b)
end subroutine show
end program main
""",'ALLOCATABLE actual')
        self.reject("""program main
call show([1,2])
contains
subroutine show(b)
integer,allocatable,intent(in)::b(:)
print *,allocated(b)
end subroutine show
end program main
""",'ALLOCATABLE variable')
        self.reject("""program main
integer,allocatable::a(:)
call change(a)
contains
subroutine change(b)
integer,allocatable,intent(in)::b(:)
allocate(b(2))
end subroutine change
end program main
""",'INTENT(IN)')

if __name__=='__main__':unittest.main()

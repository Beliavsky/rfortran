"""Allocated inputs and array results, including arrays of derived values."""
import unittest
import test_deallocate
from test_record_read import ROOT

def numeric_module(kind):
    return f'''module m
implicit none
contains
function concat(xx,yy) result(zz)
{kind},intent(in)::xx(:),yy(:)
{kind}::zz(size(xx)+size(yy))
integer::nx
nx=size(xx)
zz(:nx)=xx
zz(nx+1:)=yy
end function concat
subroutine set_alloc(xx,yy)
{kind},intent(in)::xx(:)
{kind},allocatable,intent(out)::yy(:)
allocate(yy(size(xx)))
yy=xx
end subroutine set_alloc
end module m
'''

RECORD=(ROOT/'tests/185_allocated_derived_concat.f90').read_text(encoding='utf-8').split('program test_derived_concat')[0]

class AllocatedArrayFunctions(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_all_numeric_logical_types_runtime_lengths_and_empty_inputs(self):
        for kind,values,fill,last in [('integer','1,2','3','1 2'),('real','1.0,2.0','3.0','1 2'),
                                      ('double precision','1.0d0,2.0d0','3.0d0','1 2'),
                                      ('logical','.true.,.false.','.true.','T F')]:
            with self.subTest(kind=kind):
                source=numeric_module(kind)+f'''program main
use m
implicit none
{kind},allocatable::x(:),out(:)
integer::n
do n=0,2
allocate(x(n))
x={fill}
call set_alloc(concat(x,[{values}]),out)
print *,allocated(x),allocated(out),size(out)
print *,out
deallocate(x)
end do
end program main
'''
                first='3' if kind!='logical' else 'T'
                self.check(source,f'T T 2\n{last}\nT T 3\n{first} {last}\nT T 4\n{first} {first} {last}\n')

    def test_nonpositive_extents_are_allocated_empty_arrays(self):
        for kind in ['integer','real','double precision','logical']:
            self.check(f'''{kind},allocatable::x(:)
allocate(x(-3))
print *,allocated(x),size(x)
deallocate(x)
allocate(x(0))
print *,allocated(x),size(x)
print *,x
end
''','T 0\nT 0\n\n')

    def test_runtime_array_result_larger_than_bounded_storage(self):
        self.check('''program main
integer::n
n=2000
print *,size(sequence(n)),sum(sequence(n))
contains
function sequence(n) result(out)
integer,intent(in)::n
integer::out(n)
integer::i
do i=1,n
out(i)=i
end do
end function sequence
end program main
''','2000 2001000\n')

    def test_character_repro(self):
        self.check((ROOT/'tests/184_allocated_character_concat.f90').read_text(encoding='utf-8'),'3 10\nfour|five|zero\n')

    def test_derived_repro(self):
        self.check((ROOT/'tests/185_allocated_derived_concat.f90').read_text(encoding='utf-8'),
                   '2\n1 1.5 T\n2 2.5 F\n4\n3 3.5 T\n4 4.5 F\n1 1.5 T\n2 2.5 F\n')

    def test_derived_defaults_empty_constructor_and_output(self):
        self.check(RECORD+'''program main
use derived_concat_mod,only:point,concat,set_alloc
implicit none
type(point),allocatable::x(:),out(:)
allocate(x(2))
print *,x
deallocate(x)
allocate(x(0))
call set_alloc(concat(x,[point::]),out)
print *,allocated(out),size(out)
print *,out
call set_alloc(concat(x,[point(1,1.5,.true.)]),out)
print *,out(1)
write(*,'(i0,1x,f3.1,1x,l1)')out
end program main
''','0 0 F 0 0 F\nT 0\n\n1 1.5 T\n1 1.5 T\n')

    def test_record_constructor_assignment_captures_all_components(self):
        self.check('''program main
type::pair
integer::a,b
end type pair
type(pair),allocatable::x(:)
allocate(x(1))
x(1)=pair(1,2)
x=[pair(x(1)%b,x(1)%a)]
print *,x(1)%a,x(1)%b
x(1)=pair(x(1)%b,x(1)%a)
print *,x(1)%a,x(1)%b
end program main
''','2 1\n1 2\n')

    def test_record_array_function_side_effects_run_once(self):
        self.check('''module m
integer::calls=0
type::entry
integer::id
logical::ok
end type entry
contains
function make(n) result(out)
integer,intent(in)::n
type(entry)::out(n)
integer::i
calls=calls+1
do i=1,n
out(i)=entry(i,.true.)
end do
end function make
end module m
program main
use m
integer::n
n=2
print *,make(n)
print *,calls
end program main
''','1 T 2 T\n1\n')

    def test_record_matrix_result(self):
        self.check('''module m
type::entry
integer::id
logical::ok
end type entry
contains
function copy(a) result(b)
type(entry),intent(in)::a(:,:)
type(entry)::b(size(a,1),size(a,2))
b=a
end function copy
end module m
program main
use m
type(entry),allocatable::x(:,:)
integer::i,j
allocate(x(2,2))
do j=1,2
do i=1,2
x(i,j)=entry(i+2*(j-1),.true.)
end do
end do
print *,copy(x)
print *,shape(copy(x))
end program main
''','1 T 2 T 3 T 4 T\n2 2\n')

    def test_record_rank_fifteen(self):
        colons=','.join(':' for _ in range(15))
        dims=', &\n &'.join(f'size(a,{i})' for i in range(1,16))
        extents=','.join(['2']+['1']*14)
        indices=','.join(['i']+['1']*14)
        self.check(f'''module m
type::entry
integer::id
logical::ok
end type entry
contains
function copy(a) result(b)
type(entry),intent(in)::a({colons})
type(entry)::b({dims})
b=a
end function copy
end module m
program main
use m
type(entry),allocatable::x({colons})
integer::i
allocate(x({extents}))
do i=1,2
x({indices})=entry(i,.true.)
end do
print *,copy(x)
print *,shape(copy(x))
end program main
''','1 T 2 T\n2 '+' '.join('1' for _ in range(14))+'\n')

    def test_record_allocation_bounds_evaluate_once(self):
        self.check('''module m
integer::calls=0
type::entry
integer::id=7
logical::ok=.true.
end type entry
contains
integer function extent()
calls=calls+1
extent=2
end function extent
end module m
program main
use m
type(entry),allocatable::x(:)
allocate(x(extent()))
print *,calls,size(x)
print *,x
end program main
''','1 2\n7 T 7 T\n')

    def test_uninitialized_record_component_remains_checked(self):
        self.check('''module m
type::entry
integer::id
logical::ok
end type entry
contains
function copy(a) result(b)
type(entry),intent(in)::a(:)
type(entry)::b(size(a))
b=a
end function copy
end module m
program main
use m
type(entry),allocatable::x(:)
allocate(x(1))
x(1)%id=7
print *,copy(x)
end program main
''',error='uninitialized')

if __name__=='__main__':
    unittest.main()

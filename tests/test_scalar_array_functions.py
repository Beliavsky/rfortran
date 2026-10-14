"""Scalar numeric functions receiving fixed and runtime array arguments."""
from pathlib import Path
import unittest
import test_deallocate

class ArrayFunctions(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_repro(self):
        self.check((Path(__file__).resolve().parent.parent/'xassumed_shape_function_call.f90').read_text(encoding='utf-8'), '2\n')

    def test_runtime_forwarding_empty_and_reallocation(self):
        self.check("""program main
integer,allocatable::a(:)
integer::n,i
do n=0,3
 allocate(a(-2:n-3))
 do i=1,n
  a(i-3)=i
 end do
 print *,total(a),outer(a),first_bound(a),last_bound(a)
 deallocate(a)
end do
contains
pure function total(x) result(y)
integer,intent(in)::x(:)
integer::y
y=sum(x)
end function total
pure function outer(x) result(y)
integer,intent(in)::x(:)
integer::y
y=total(x)+size(x)
end function outer
pure function first_bound(x) result(y)
integer,intent(in)::x(:)
integer::y
y=lbound(x,1)
end function first_bound
pure function last_bound(x) result(y)
integer,intent(in)::x(:)
integer::y
y=ubound(x,1)
end function last_bound
end program main
""", '0 0 1 0\n1 2 1 1\n3 5 1 2\n6 9 1 3\n')

    def test_runtime_expression_and_fixed_section(self):
        self.check("""program main
integer,allocatable::a(:)
integer::n,b(6)
n=3
allocate(a(n))
a=2
b=[1,2,3,4,5,6]
print *,total(a+1),total([1,2,3]),total([integer::]),total(b(1:6:2))
deallocate(a)
contains
pure function total(x) result(y)
integer,intent(in)::x(:)
integer::y
y=sum(x)
end function total
end program main
""", '9 6 0 9\n')

    def test_multidimensional(self):
        self.check("""program main
real(8),allocatable::a(:,:),b(:,:,:)
integer::n
n=2
allocate(a(n,3),b(n,2,3))
a=2.0d0
b=3.0d0
print *,total2(a),total3(b)
deallocate(a,b)
contains
pure function total2(x) result(y)
real(8),intent(in)::x(:,:)
real(8)::y
y=sum(x)
end function total2
pure function total3(x) result(y)
real(8),intent(in)::x(:,:,:)
real(8)::y
y=sum(x)
end function total3
end program main
""", '12 36\n')

    def test_early_return_and_keyword_arguments(self):
        self.check("""program main
print *,has_value(value=2,values=[1,2,3]),has_value(4,[1,2,3])
contains
pure function has_value(value,values) result(tf)
integer,intent(in)::value,values(:)
logical::tf
integer::i
tf=.false.
do i=1,size(values)
 if(values(i)==value) then
  tf=.true.
  return
 end if
end do
end function has_value
end program main
""", 'T F\n')

    def test_unused_input_does_not_read_uninitialized_elements(self):
        self.check("""program main
integer,allocatable::a(:)
integer::n
n=2
allocate(a(n))
print *,count_elements(a)
deallocate(a)
contains
pure function count_elements(x) result(y)
integer,intent(in)::x(:)
integer::y
y=size(x)
end function count_elements
end program main
""", '2\n')

    def test_rank_fifteen(self):
        deferred=','.join([':']*15)
        bounds=','.join(['n']+['1']*14)
        self.check(f"""program main
integer,allocatable::a({deferred})
integer::n
n=2
allocate(a({bounds}))
a=5
print *,total(a)
deallocate(a)
contains
pure function total(x) result(y)
integer,intent(in)::x({deferred})
integer::y
y=sum(x)
end function total
end program main
""",'10\n')

    def test_character_forwarding(self):
        text=(Path(__file__).resolve().parent.parent/'xassumed_shape_function_call.f90').read_text(encoding='utf-8')
        text=text.replace('integer,intent(in)', 'character(len=1),intent(in)').replace('call check_values([1,2,3],[1,3])', 'call check_values(["a","b","c"],["a","c"])')
        self.check(text, 'b\n')

    def test_element_read_checks_initialization(self):
        self.check("""program main
integer,allocatable::a(:)
integer::n
n=2
allocate(a(n))
a(1)=1
print *,total(a)
contains
pure function total(x) result(y)
integer,intent(in)::x(:)
integer::y
y=sum(x)
end function total
end program main
""",error='uninitialized')

if __name__=='__main__':unittest.main()

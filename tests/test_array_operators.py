"""Defined operators with read-only arrays and call-site function specialization."""
import unittest
import test_deallocate
from test_record_read import ROOT

MODULE = """module membership_mod
interface operator(.notin.)
module procedure int_not_in_vec
end interface
contains
pure function int_not_in_vec(i,ivec) result(tf)
integer,intent(in)::i,ivec(:)
logical::tf
tf=all(ivec/=i)
end function int_not_in_vec
end module membership_mod
"""


class ArrayOperators(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_reproducer(self):
        self.check((ROOT/'tests/programs/xnotin.f90').read_text(encoding='utf-8'), 'F T\n')

    def test_whole_arrays_sections_constructors_and_empty_arrays(self):
        self.check(MODULE+"""use membership_mod
integer::values(5)
values=[1,2,3,4,5]
print *,2 .notin. values,6 .notin. values
print *,2 .notin. values(1:5:2),4 .notin. values(2:4:2)
print *,3 .notin. [1,3,5],7 .notin. [integer::]
print *,1 .notin. [2,3],1 .notin. (values+1)
print *,int_not_in_vec(2,values),int_not_in_vec(7,[1,2]),int_not_in_vec(1,[integer::])
end
""", 'F T\nT F\nF T\nT T\nF T T\n')

    def test_array_results_and_nested_function_calls(self):
        self.check("""module membership_mod
interface operator(.notin.)
module procedure one,vector
end interface
contains
pure function one(i,values) result(tf)
integer,intent(in)::i,values(:)
logical::tf
tf=all(values/=i)
end function one
pure function vector(left,right) result(tf)
integer,intent(in)::left(:),right(:)
logical::tf(size(left))
integer::i
do i=1,size(left)
tf(i)=one(left(i),right)
end do
end function vector
end module membership_mod
use membership_mod
print *,[1,2,3] .notin. [2,4]
print *,[1,2] .notin. [integer::]
print *,size([integer::] .notin. [1,2])
print *,all([1,3] .notin. [2,4])
end
""", 'T F T\nT T\n0\nT\n')

    def test_real_and_logical_array_operands(self):
        self.check("""module arrays
interface operator(.has.)
module procedure has_real,has_double,has_logical
end interface
contains
pure logical function has_real(values,target)
real,intent(in)::values(:),target
has_real=any(values==target)
end function has_real
pure logical function has_double(values,target)
double precision,intent(in)::values(:),target
has_double=any(values==target)
end function has_double
pure logical function has_logical(values,target)
logical,intent(in)::values(:),target
has_logical=any(values.eqv.target)
end function has_logical
end module arrays
use arrays
print *,[1.0,2.0] .has. 2.0,[1.0,2.0] .has. 3.0
print *,[1.0d0,2.0d0] .has. 2.0d0,[1.0d0,2.0d0] .has. 3.0d0
print *,[.true.,.true.] .has. .false.,[.true.,.false.] .has. .false.
end
""", 'T F\nT F\nF T\n')

    def test_integer_kind_is_preserved(self):
        self.check(MODULE.replace('integer,intent(in)::i,ivec(:)',
                                 'integer(8),intent(in)::i,ivec(:)')+"""use membership_mod
print *,10000000000_8 .notin. [10000000000_8,10000000001_8]
print *,10000000002_8 .notin. [10000000000_8,10000000001_8]
end
""", 'F\nT\n')

    def test_scalar_evaluation_and_loop_condition_reexecution(self):
        self.check(MODULE+"""use membership_mod
integer::values(1),i
values=1
print *,next_value() .notin. values
print *,next_value() .notin. values
values=3
i=1
do while(i .notin. values)
i=i+1
end do
print *,i
contains
integer function next_value() result(n)
integer,save::calls=0
calls=calls+1
n=calls
end function next_value
end
""", 'F\nT\n3\n')

    def test_input_initialization_and_type_checks(self):
        self.reject(MODULE+'use membership_mod\ninteger::values(3)\nvalues(1)=1\nprint *,2 .notin. values\nend\n', 'uninitialized')
        self.reject(MODULE+'use membership_mod\nprint *,2 .notin. [1.0,2.0]\nend\n', 'no matching operator(.notin.)')

    def test_optional_function_variants_keep_their_shapes(self):
        self.check("""integer::values(3)
values=[1,2,3]
print *,total(values),total()
contains
pure integer function total(x) result(n)
integer,intent(in),optional::x(3)
if(present(x))then
n=sum(x)
else
n=-1
end if
end function total
end
""", '6 -1\n')

    def test_array_result_from_scalar_arguments(self):
        self.check("""module arrays
interface operator(.pair.)
module procedure pair
end interface
contains
pure function pair(n) result(out)
integer,intent(in)::n
integer::out(2)
out=[n,n]
end function pair
end module arrays
use arrays
print *,.pair. 1
end
""", '1 1\n')


if __name__=='__main__':
    unittest.main()

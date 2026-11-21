"""Elemental operators on scalar and array operands, including derived types."""
import unittest
import test_deallocate
from test_record_read import ROOT

MODULE = """module elemental_operator_mod
implicit none
type :: token
integer :: value
logical :: flag
end type
interface operator(>)
module procedure gt_token
end interface
interface operator(==)
module procedure eq_token
end interface
interface operator(+)
module procedure add_token
end interface
interface operator(-)
module procedure negate_token
end interface
interface operator(.not.)
module procedure not_token
end interface
interface operator(.same.)
module procedure eq_token
end interface
interface operator(.year.)
module procedure get_value
end interface
contains
elemental logical function gt_token(a,b) result(y)
type(token),intent(in)::a,b
y=a%value>b%value
end function
elemental logical function eq_token(a,b) result(y)
type(token),intent(in)::a,b
y=a%value==b%value .and. (a%flag.eqv.b%flag)
end function
elemental integer function add_token(a,b) result(y)
type(token),intent(in)::a,b
y=a%value+b%value
end function
elemental integer function negate_token(a) result(y)
type(token),intent(in)::a
y=-a%value
end function
elemental logical function not_token(a) result(y)
type(token),intent(in)::a
y=.not.a%flag
end function
elemental integer function get_value(a) result(y)
type(token),intent(in)::a
y=a%value
end function
end module
"""

VECTOR = """use elemental_operator_mod
implicit none
type(token)::a(3),b(3),s
a%value=[1,2,3]
b%value=[0,2,4]
s%value=2
a%flag=.true.
b%flag=.true.
s%flag=.true.
"""


class ElementalOperators(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_xacv_reproducer(self):
        self.check((ROOT/'tests/programs/xacv_elemental_derived_comparison.f90').read_text(encoding='utf-8'), 'T\n')

    def test_both_scalar_orders_and_array_array(self):
        self.check(MODULE + VECTOR + """print *,a>s
print *,s>a
print *,a>b
print *,a==s,s==a,a==b
print *,all(a==a),any(s==a),count(a>s)
end
""", 'F F T\nT F F\nT F F\nF T F F T F F T F\nT T 1\n')

    def test_defined_arithmetic_and_unary_operators(self):
        self.check(MODULE + VECTOR + """print *,a+b,s+a,a+s
print *,-a,.year.a,.not.a
print *,a .same. b
print *,sum(a+b),sum((a+b)+get_value(s))
end
""", '1 4 7 3 4 5 3 4 5\n-1 -2 -3 1 2 3 F F F\nF T F\n12 18\n')

    def test_sections_array_element_and_empty(self):
        self.check(MODULE + VECTOR.replace('type(token)::a(3),b(3),s','type(token)::a(3),b(3),s,empty(0)') + """print *,a(3:1:-1)>s
print *,s>a([3,1])
print *,a>a(2)
print *,any(empty==s),all(s==empty),count(empty>s)
end
""", 'T F F\nF T\nF F T\nF T 0\n')

    def test_matrix_and_rank_three_results(self):
        program = """use elemental_operator_mod
implicit none
type(token)::a(2,3),b(2,3),c(2,1,3),s
a%value=reshape([1,2,3,4,5,6],[2,3])
b%value=3
c%value=reshape([1,2,3,4,5,6],[2,1,3])
s%value=3
a%flag=.true.
b%flag=.true.
c%flag=.true.
s%flag=.true.
print *,shape(a>s),shape(s>c)
print *,a>s,s>a,a>b
print *,sum(c+s),count(c>s)
print *,a(:,2:3)>s
end
"""
        self.check(MODULE + program, '2 3 2 1 3\nF F F T T T T T F F F F F F F T T T\n39 3\nF T T T\n')

    def test_axis_conformance_not_just_total_size(self):
        program = """use elemental_operator_mod
implicit none
type(token)::a(2,3),b(3,2)
a%value=1
b%value=2
a%flag=.true.
b%flag=.true.
print *,a>b
end
"""
        self.reject(MODULE + program, 'nonconforming')

    def test_runtime_allocated_arrays(self):
        program = """use elemental_operator_mod
implicit none
integer::n
type(token),allocatable::a(:),b(:)
read *,n
allocate(a(n),b(n))
a%value=1
b%value=2
a%flag=.true.
b%flag=.true.
print *,all(b>a),any(a==b),count(b>a)
end
"""
        self.check(MODULE + program, 'T F 3\n', data='3\n')

    def test_direct_elemental_calls_both_orders(self):
        self.check(MODULE + VECTOR + """print *,eq_token(a,s),eq_token(s,a),gt_token(a,b)
end
""", 'F T F F T F T F F\n')

    def test_rank_fifteen(self):
        dims = ",".join(["1"] * 14 + ["2"])
        program = f"""use elemental_operator_mod
implicit none
type(token)::a({dims}),s
a%value=3
a%flag=.true.
s%value=3
s%flag=.true.
print *,all(a==s),count(s==a),shape(a>s)
end
"""
        self.check(MODULE + program, 'T 2 ' + ' '.join(['1'] * 14 + ['2']) + '\n')

    def test_runtime_matrix_conformance(self):
        program = """use elemental_operator_mod
implicit none
integer::n
type(token),allocatable::a(:,:),b(:,:)
read *,n
allocate(a(n,2),b(n,2))
a%value=1
b%value=2
a%flag=.true.
b%flag=.true.
print *,all(b>a),count(b>a)
print *,all(b(:,2:)>a(:,2:)),count(b(:,2:)>a(:,2:))
end
"""
        self.check(MODULE + program, 'T 6\nT 3\n', data='3\n')
        bad = program.replace('b(n,2)', 'b(2,n)')
        self.check(MODULE + bad, error='conform', data='3\n', reference=False)

    def test_unused_components_need_not_be_initialized(self):
        program = """use elemental_operator_mod
implicit none
type(token)::a(2),s
a%value=[1,3]
s%value=2
print *,a>s
end
"""
        self.check(MODULE + program, 'F T\n')

    def test_operator_in_assumed_shape_procedure(self):
        module = MODULE.replace('end module', """pure logical function ascending(values) result(y)
type(token),intent(in)::values(:)
y=all(values(2:) > values(:size(values)-1))
end function
end module""")
        self.check(module + VECTOR + 'print *,ascending(a),ascending(b)\nend\n', 'T T\n')

    def test_unused_operand_still_conforms(self):
        module = MODULE.replace('y=a%value>b%value', 'y=a%value>0', 1)
        self.check(module + VECTOR + 'print *,a>b\nend\n', 'T T T\n')
        bad = VECTOR.replace('b(3)', 'b(2)').replace('b%value=[0,2,4]', 'b%value=[0,2]')
        self.reject(module + bad + 'print *,a>b\nend\n', 'conform')

    def test_invalid_derived_elemental_dummy(self):
        pointer = MODULE.replace('type(token),intent(in)::a,b', 'type(token),pointer,intent(in)::a,b', 1)
        self.reject(pointer + VECTOR + 'print *,a>s\nend\n', 'attribute `pointer`')

    def test_character_component_and_result(self):
        program = """module labels
implicit none
type::item
character(len=2)::text
end type
interface operator(.label.)
module procedure label
end interface
contains
elemental function label(a) result(text)
type(item),intent(in)::a
character(len=2)::text
text=a%text
end function
end module
use labels
implicit none
type(item)::a(3)
a%text=['ab','cd','ef']
print '(3(a,1x))',.label.a
end
"""
        self.check(program, 'ab cd ef\n')

    def test_impure_elemental_operator_runs_once_per_element(self):
        module = MODULE.replace('type :: token', 'integer::calls=0\ntype :: token', 1)
        module = module.replace('elemental logical function gt_token', 'impure elemental logical function gt_token', 1)
        module = module.replace('y=a%value>b%value', 'calls=calls+1\ny=a%value>b%value', 1)
        self.check(module + VECTOR + 'print *,a>s\nprint *,calls\nend\n', 'F F T\n3\n')

    def test_builtin_type_defined_operator(self):
        program = """module m
interface operator(.blend.)
module procedure blend
end interface
contains
elemental integer function blend(a,b) result(y)
integer,intent(in)::a,b
y=a+b
end function
end module
use m
integer::a(3)
a=[1,2,3]
print *,a .blend. 10,10 .blend. a,a .blend. a
end
"""
        self.check(program, '11 12 13 11 12 13 2 4 6\n')

    def test_non_elemental_does_not_broadcast(self):
        self.reject(MODULE.replace('elemental logical function gt_token','pure logical function gt_token') + VECTOR + 'print *,a>s\nend\n', 'no matching operator(>)')

    def test_mismatched_rank_rejected(self):
        program = """use elemental_operator_mod
implicit none
type(token)::a(2),b(2,2)
a%value=1
b%value=2
a%flag=.true.
b%flag=.true.
print *,a>b
end
"""
        self.reject(MODULE + program, 'no matching operator(>)')


    def test_derived_comparisons_and_derived_results_in_output(self):
        # Defined comparison operators on a derived type, for scalars and
        # arrays, with generic MINVAL and MAXVAL extended to the type. Their
        # results, and MERGE of records, in output lists were rejected:
        # "derived-valued expression `minval_date` requires a derived
        # assignment or argument".
        self.check("""module dates
implicit none
type date
   integer :: month = 0, day = 0, year = 0
end type
interface operator (>)
   module procedure date_gt
end interface
interface operator (<)
   module procedure date_lt
end interface
interface operator (>=)
   module procedure date_ge
end interface
interface operator (<=)
   module procedure date_le
end interface
interface operator (==)
   module procedure date_eq
end interface
interface operator (/=)
   module procedure date_ne
end interface
interface minval
   module procedure minval_date
end interface
interface maxval
   module procedure maxval_date
end interface
contains
elemental integer function key(d)
type(date), intent(in) :: d
key = d%year*10000 + d%month*100 + d%day
end function
elemental logical function date_gt(a, b)
type(date), intent(in) :: a, b
date_gt = key(a) > key(b)
end function
elemental logical function date_lt(a, b)
type(date), intent(in) :: a, b
date_lt = b > a
end function
elemental logical function date_ge(a, b)
type(date), intent(in) :: a, b
date_ge = .not. (a < b)
end function
elemental logical function date_le(a, b)
type(date), intent(in) :: a, b
date_le = .not. (a > b)
end function
elemental logical function date_eq(a, b)
type(date), intent(in) :: a, b
date_eq = key(a) == key(b)
end function
elemental logical function date_ne(a, b)
type(date), intent(in) :: a, b
date_ne = .not. (a == b)
end function
elemental function min_date(a, b) result(m)
type(date), intent(in) :: a, b
type(date) :: m
m = merge(a, b, b > a)
end function
pure function minval_date(x) result(y)
type(date), intent(in) :: x(:)
type(date) :: y
integer :: i
if (size(x) < 1) return
y = x(1)
do i = 2, size(x)
   y = min_date(y, x(i))
end do
end function
pure function maxval_date(x) result(y)
type(date), intent(in) :: x(:)
type(date) :: y
integer :: i
if (size(x) < 1) return
y = x(1)
do i = 2, size(x)
   if (x(i) > y) y = x(i)
end do
end function
end module dates

program main
use dates
implicit none
character(len=*), parameter :: fmt_cdate = "(a20,':',2(1x,2(i2.2,'/'),i4.4))"
type(date), allocatable :: d(:)
type(date) :: lo, hi, t
integer :: i, j
d = [date(3, 15, 2021), date(1, 2, 2020), date(12, 31, 2021), date(1, 2, 2020)]
write (*, fmt_cdate) "first, last dates", minval(d), maxval(d)
print *, minval(d), maxval(d(2:3))
lo = minval(d)
hi = maxval(d)
print *, lo < hi, lo > hi, lo <= hi, lo >= hi, lo == hi, lo /= hi
print *, d(2) == d(4), d(1) /= d(3)
print *, d < d(1)
print *, d >= date(1, 1, 2021)
print *, count(d < hi), any(d == lo), all(d <= hi)
if (d(1) > d(2) .and. .not. (d(3) < d(1))) print *, 'ordered'
! insertion sort with the defined operators
do i = 2, size(d)
   t = d(i)
   j = i - 1
   do while (j >= 1)
      if (.not. (d(j) > t)) exit
      d(j+1) = d(j)
      j = j - 1
   end do
   d(j+1) = t
end do
print *, d
print *, merge(lo, hi, lo > hi)
print *, key(minval(d)), key(min_date(d(4), d(1)))
end program
""", "   first, last dates: 01/02/2020 12/31/2021\n 1 2 2020 12 31 2021\n T F T F F T\n T T\n F T F T\n T F T F\n 3 T T\n ordered\n 1 2 2020 1 2 2020 3 15 2021 12 31 2021\n 12 31 2021\n 20200102 20200102\n")


if __name__ == '__main__':
    unittest.main()

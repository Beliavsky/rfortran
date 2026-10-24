"""Specialize array procedure calls inside SELECT CASE branches."""
import unittest

import test_deallocate
from test_record_read import ROOT


REPRO = (ROOT / 'tests/programs/xacv_median_select_case.f90').read_text(encoding='utf-8')


class SelectCaseCalls(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_median_repro_and_default(self):
        self.check(REPRO, '3.00\n')
        self.check(REPRO.replace("stat_vec('median',", "stat_vec('sum',"), '6.00\n')

    def test_direct_function_result_in_case_and_default(self):
        start = REPRO.index('xcopy=xx')
        end = REPRO.index('end function', start)
        source = REPRO[:start] + 'xmed=sum(xx)/size(xx)\n' + REPRO[end:]
        self.check(source, '3.00\n')
        source = source.replace('xstat=sum(xx)', 'xstat=median(xx)')
        self.check(source.replace("stat_vec('median',", "stat_vec('other',"), '3.00\n')

    def test_nested_cases_runtime_arrays_and_branch_side_effects(self):
        self.check('''module select_calls_mod
implicit none
integer::calls=0
contains
function total(x) result(y)
integer,intent(in)::x(:)
integer::y
calls=calls+1
y=sum(x)
end function
end module
program main
use select_calls_mod
implicit none
integer,allocatable::x(:)
integer::n,choice,y
read *,n
allocate(x(-1:n-2))
x=2
do choice=0,2
   select case(choice)
   case(1)
      select case(n)
      case(0)
         y=total(x)+10
      case default
         y=total(x)+20
      end select
   case(2)
      y=total(x)+30
   case default
      y=total(x)
   end select
   print *,y,calls
end do
end
''', '6 1\n26 2\n36 3\n', data='3\n')

    def test_array_valued_function_in_branches(self):
        self.check('''implicit none
integer::choice,x(3),y(3)
x=[1,2,3]
do choice=1,2
   select case(choice)
   case(1)
      y=twice(x)
   case default
      y=twice(x)+1
   end select
   print *,y
end do
contains
function twice(x) result(y)
integer,intent(in)::x(:)
integer::y(size(x))
y=2*x
end function
end
''', '2 4 6\n3 5 7\n')

    def test_array_subroutine_output_in_branches(self):
        self.check('''implicit none
integer::choice,x(2),y
x=[2,4]
do choice=1,2
   select case(choice)
   case(1)
      call total(x,y)
   case default
      call total(x+1,y)
   end select
   print *,y
end do
contains
subroutine total(x,y)
integer,intent(in)::x(:)
integer,intent(out)::y
y=sum(x)
end subroutine
end
''', '6\n8\n')

    def test_uninitialized_argument_is_still_rejected(self):
        self.reject('''implicit none
integer::choice,x(2),y
read *,choice
select case(choice)
case(1)
   y=total(x)
case default
   y=0
end select
print *,y
contains
integer function total(x) result(y)
integer,intent(in)::x(:)
y=sum(x)
end function
end
''', 'uninitialized variable')


if __name__ == '__main__':
    unittest.main()

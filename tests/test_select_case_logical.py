"""LOGICAL selectors must dispatch correctly in both serial native and interpreter modes."""
import unittest
import test_fortran2023
import test_deallocate


class LogicalSelectCase(unittest.TestCase):
    check=test_fortran2023.Fortran2023.check
    reject=test_deallocate.Deallocate.reject

    def test_variables_expressions_and_constants(self):
        for declaration in ('logical','logical(kind=4)','logical(kind=kind(.true.))'):
            with self.subTest(declaration=declaration):
                self.check(f'''implicit none
{declaration}::flag
logical,parameter::yes=.not..false.,no=.false.
integer::i,n
n=0
do i=1,2
 flag=i==1
 select case(flag)
 case(yes)
  n=n+1
 case(no)
  n=n+2
 end select
 select case(i==1)
 case(.true.)
  n=n+10
 case default
  n=n+20
 end select
end do
select case(.false.)
case(.true.)
 stop 1
case default
 n=n+100
end select
if(n/=133) stop 2
print *,"PASS"
end
''',' PASS\n')

    def test_function_selector_evaluated_once(self):
        self.check('''module choices
implicit none
integer::calls=0
contains
logical function next_choice()
calls=calls+1
next_choice=mod(calls,2)==1
end function
end module
program p
use choices
implicit none
integer::i,n
n=0
do i=1,3
 select case(next_choice())
 case(.true.)
  n=n+1
 case(.false.)
  n=n+10
 end select
end do
if(calls/=3.or.n/=12) stop 1
print *,"PASS"
end
''',' PASS\n')

    def test_lists_no_match_and_branch_changes_selector(self):
        self.check('''implicit none
logical::flag
integer::n
flag=.true.
n=0
select case(flag)
case(.true.)
 flag=.false.
 n=1
case(.false.)
 n=2
end select
select case(flag)
case(.true.)
 n=99
end select
select case(flag)
case(.true.,.false.)
 n=n+10
end select
if(n/=11) stop 1
print *,"PASS"
end
''',' PASS\n')

    def test_overlapping_cases_rejected(self):
        self.reject('''implicit none
logical::flag
flag=.true.
select case(flag)
case(.true.)
 print *,1
case(.not..false.)
 print *,2
end select
end
''','overlapping LOGICAL CASE selectors')

    def test_logical_ranges_rejected(self):
        self.reject('''implicit none
logical::flag
flag=.true.
select case(flag)
case(.false.:.true.)
 print *,1
end select
end
''','LOGICAL CASE selectors cannot be ranges')


if __name__=='__main__':
    unittest.main()

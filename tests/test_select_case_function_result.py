"""Semantic checking of function results inside SELECT CASE branches."""
from pathlib import Path
import unittest
import test_deallocate as helpers
REPRO=(Path(__file__).parent/'programs/xselect_case_function_result.f90').read_text()

class SelectCaseFunctionResult(unittest.TestCase):
    check=helpers.Deallocate.check
    reject=helpers.Deallocate.reject

    def test_calendar_repro(self):
        self.check(REPRO,"29 28\n30 31 0\n")

    def test_non_elemental_function(self):
        self.check(REPRO.replace('pure elemental ',''),"29 28\n30 31 0\n")

    def test_explicit_result(self):
        source=REPRO.replace('function days_in_month(year, month)', 'function days_in_month(year, month) result(ndays)')
        source=source.replace('days_in_month =', 'ndays =')
        self.check(source,"29 28\n30 31 0\n")

    def test_nested_case_and_block(self):
        source=REPRO.replace('days_in_month = 31', """block
select case(year)
case(2026)
days_in_month=31
case default
days_in_month=30
end select
end block""")
        self.check(source,"29 28\n30 31 0\n")

    def test_assignment_in_default_only(self):
        self.check("""program main
implicit none
print *,value(3)
contains
integer function value(n)
integer,intent(in)::n
select case(n)
case(1)
error stop 'unexpected'
case default
value=42
end select
end function
end
""","42\n")

    def test_branch_assignment_type_checked(self):
        for old in ['days_in_month = 31','days_in_month = 0']:
            with self.subTest(old=old):
                self.reject(REPRO.replace(old,"days_in_month = 'invalid'"), 'type mismatch')

    def test_missing_result_still_rejected(self):
        self.reject("""program main
implicit none
print *,missing(1)
contains
integer function missing(n)
integer,intent(in)::n
select case(n)
case(1)
print *,n
case default
print *,0
end select
end function
end
""", 'read of uninitialized variable `missing`')

if __name__=='__main__':
    unittest.main()

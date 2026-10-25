"""Derived arrays and their components inside SELECT CASE."""
from pathlib import Path
import unittest
import test_deallocate as helpers

REPRO = (Path(__file__).parent / 'programs/xderived_array_select_case.f90').read_text()

class DerivedArraySelectCase(unittest.TestCase):
    check = helpers.Deallocate.check

    def test_selector_and_function_actuals(self):
        self.check(REPRO, "1 2 3\n")

    def test_default_branch(self):
        self.check(REPRO.replace('case(2)', 'case(9)'), "3\n")

    def test_nested_select_and_empty_result(self):
        source = REPRO.replace('idx_out=join(self%index,other%index)',
            """select case(size(other%index))
case(1)
idx_out=[date::]
case default
idx_out=join(self%index,other%index)
end select""")
        source = source.replace('print *,c%index%day', 'print *,allocated(c%index),size(c%index)')
        self.check(source, "T 0\n")

    def test_array_result_return_in_case(self):
        source = REPRO.replace('allocate(c(size(a)+size(b)))',
            """select case(size(b))
case(1)
allocate(c(size(b)))
c=b
return
case default
allocate(c(size(a)+size(b)))
end select""")
        self.check(source, "3\n")

    def test_array_result_return_in_default(self):
        source = REPRO.replace('allocate(c(size(a)+size(b)))',
            """select case(size(b))
case(9)
allocate(c(size(a)+size(b)))
case default
allocate(c(size(a)))
c=a
return
end select""")
        self.check(source, "1 2\n")

    def test_block_local_record_array(self):
        source = REPRO.replace('idx_out=join(self%index,other%index)',
            """block
type(date)::m(2)
m=[date(5),date(6)]
print *,m%day
idx_out=join(self%index,other%index)
end block""")
        self.check(source, "5 6\n1 2 3\n")

if __name__ == '__main__':
    unittest.main()

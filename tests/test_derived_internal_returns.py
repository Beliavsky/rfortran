"""Internal function results survive conversion of a derived-result host."""
from pathlib import Path
import unittest
import test_deallocate as helpers

REPRO = (Path(__file__).parent / 'programs/xreindex_internal_result.f90').read_text()

class DerivedInternalReturns(unittest.TestCase):
    check = helpers.Deallocate.check

    def test_binary_search(self):
        self.check(REPRO, "0 1 0 2 3 0\n0\n0 0\n")

    def test_implicit_result_name(self):
        source = REPRO.replace('function search(a,x) result(pos)', 'function search(a,x)')
        source = source.replace('pos=0', 'search=0').replace('pos=mid', 'search=mid')
        self.check(source, "0 1 0 2 3 0\n0\n0 0\n")

    def test_impure_host_and_internal_function(self):
        self.check(REPRO.replace('pure ', ''), "0 1 0 2 3 0\n0\n0 0\n")

if __name__ == '__main__':
    unittest.main()

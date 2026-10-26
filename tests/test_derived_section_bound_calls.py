"""Derived argument association inside section bounds and strides."""
from pathlib import Path
import unittest
import test_deallocate as helpers

SOURCE=(Path(__file__).parent/'programs/xderived_constructor_stride.f90').read_text()
EXPECTED='1 3 5\n1 3 5 6 8 10\n'

class DerivedSectionBoundCalls(unittest.TestCase):
    check=helpers.Deallocate.check

    def test_constructor_upper_bound(self):
        self.check(SOURCE,EXPECTED)

    def test_all_bounds(self):
        self.check(SOURCE.replace('1:nrow(x):stride','nrow(x)-4:nrow(x):nrow(x)-3'),EXPECTED)

    def test_reverse(self):
        # Installed gfortran 17 fails allocating the negative-stride constructor.
        # Check the known element order in the interpreter and native O0/O2.
        self.check(SOURCE.replace('1:nrow(x):stride','nrow(x):1:stride').replace('stride=2','stride=-2'),
                   '5 3 1\n5 3 1 10 8 6\n',reference=False)

    def test_empty(self):
        self.check(SOURCE.replace('1:nrow(x):stride','nrow(x)+1:nrow(x):stride').replace('print *,y%index','print *,size(y%index)').replace('print *,y%values','print *,shape(y%values)'),
                   '0\n0 2\n')

    def test_ordinary_function(self):
        self.check(SOURCE.replace('elemental function subset','function subset'),EXPECTED)

    def test_repeated_assignment(self):
        self.check(SOURCE.replace('y=subset(x,stride)','y=subset(x,1)\ny=subset(x,stride)'),EXPECTED)

if __name__=='__main__':
    unittest.main()

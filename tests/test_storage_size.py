"""STORAGE_SIZE of intrinsic variables, arrays, elements and derived types,
with gfortran's layout: components aligned to their own alignment and the
whole padded to the largest. Checked in the interpreter, at -O0 and -O2, and
against gfortran. Run after cargo build --bin rfort: python tests/test_storage_size.py
"""
import unittest
import test_deallocate


class StorageSize(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_intrinsic_and_derived_layouts(self):
        self.check('module m\nimplicit none\ninteger, parameter :: dp = kind(1.0d0)\ntype :: t\n   integer :: i\n   real(kind=dp) :: r(2)\n   character(len=3) :: c\n   logical :: l\n   integer(kind=8) :: big\nend type\ntype :: pad\n   character(len=3) :: c\n   integer :: i\nend type\ntype :: pad8\n   integer(kind=8) :: a\n   integer(kind=1) :: b\nend type\ntype :: nest\n   integer(kind=2) :: s\n   type(pad8) :: p(2)\n   character :: tail\nend type\ntype :: rec\n   integer :: i\n   real :: r(2)\nend type\nend module\nprogram p\nuse m\nimplicit none\ntype(t) :: x(2), y\ntype(pad) :: q\ntype(pad8) :: w\ntype(nest) :: v\ntype(rec) :: rr(3)\nreal(kind=dp) :: d\ninteger(kind=2) :: h(4)\ncharacter(len=7) :: s\nlogical :: flag\ncomplex(kind=dp) :: zz\nprint *, storage_size(x), storage_size(y), storage_size(q), storage_size(w), storage_size(v)\nprint *, storage_size(rr), storage_size(rr(2)), storage_size(d), storage_size(h), storage_size(h(1))\nprint *, storage_size(s), storage_size(flag), storage_size(zz), storage_size(x, kind=8), kind(storage_size(x, kind=8))\nend program\n', ' 320 320 64 128 384\n 96 96 64 16 16\n 56 32 128 320 8\n')

    def test_named_argument_in_expression(self):
        self.check("""type :: rec
   integer :: i
   real :: r(2)
end type
type(rec) :: x(3)
integer :: bits
bits = storage_size(a=x) * size(x)
print *, bits, storage_size(x(1), kind=8) / 8
end
""", " 288 12\n")


if __name__ == "__main__":
    unittest.main(verbosity=2)

"""Known ELEMENTAL function results in specialized scalar actual expressions."""
from pathlib import Path
import unittest
import test_deallocate as helpers
REPRO = (Path(__file__).parent / 'programs/xelemental_function_actual.f90').read_text()

class ElementalFunctionActuals(unittest.TestCase):
    check = helpers.Deallocate.check
    reject = helpers.Deallocate.reject

    def test_derived_result_repro(self):
        self.check(REPRO, "10\n")

    def test_elemental_array_call(self):
        source=REPRO.replace('type(item)::x,y', 'type(item)::x(2),y(2)')
        source=source.replace('x=item(7)', 'x=[item(7),item(9)]')
        self.check(source, "10 12\n")

    def test_evaluated_once(self):
        source=REPRO.replace('implicit none\ntype item', 'implicit none\ninteger::calls=0\ntype item')
        source=source.replace('pure elemental', 'impure elemental')
        source=source.replace('n=x%value', 'calls=calls+1\nn=x%value')
        source=source.replace('print *,y%value', 'print *,y%value,calls')
        self.check(source, "10 1\n")

    def test_nested_numeric_and_logical_results(self):
        self.check("""program main
implicit none
integer::i
real(kind=kind(1.d0))::r
logical::b
call puti(twice(twice(2))+1,i)
call putr(real_value(2.d0)*2.d0,r)
call putb(positive(1).and.positive(2),b)
print *,i,r,b
contains
pure elemental integer function twice(n)
integer,intent(in)::n
twice=2*n
end function
pure elemental real(kind=kind(1.d0)) function real_value(x)
real(kind=kind(1.d0)),intent(in)::x
real_value=x+0.5d0
end function
pure elemental logical function positive(n)
integer,intent(in)::n
positive=n>0
end function
pure elemental subroutine puti(x,y)
integer,intent(in)::x
integer,intent(out)::y
y=x
end subroutine
pure elemental subroutine putr(x,y)
real(kind=kind(1.d0)),intent(in)::x
real(kind=kind(1.d0)),intent(out)::y
y=x
end subroutine
pure elemental subroutine putb(x,y)
logical,intent(in)::x
logical,intent(out)::y
y=x
end subroutine
end
""", "9 5 T\n")

    def test_mismatched_function_result_rejected(self):
        source=REPRO.replace('pure elemental integer function value_of', 'pure elemental real function value_of')
        self.reject(source, 'type mismatch')

if __name__ == '__main__':
    unittest.main()

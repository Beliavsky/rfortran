"""Extend intrinsic operators for CHARACTER operands without replacing arithmetic."""
import unittest
import test_deallocate


MODULE = """module strings
interface operator(+)
module procedure join_chars
end interface
contains
elemental function join_chars(a,b) result(out)
character(*),intent(in)::a,b
character(len=len(a)+len(b))::out
out=trim(a)//trim(b)
end function join_chars
end module strings
"""


class CharacterOperators(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_character_extension_preserves_numeric_addition(self):
        self.check(MODULE+"""use strings,only:operator(+)
character(8)::s
s='ab '+' cd'
print *, '['//s//']',2+3,1.5+2.5
end
""", '[ab cd   ] 5 4\n')

    def test_nested_character_operator_expressions(self):
        self.check(MODULE+"""use strings
character(10)::s
s=('a '+' b')+'c '
print *,'['//s//']'
end
""", '[a bc      ]\n')

    def test_cannot_replace_intrinsic_arithmetic(self):
        invalid=MODULE.replace('character(*),intent(in)::a,b','integer,intent(in)::a,b').replace('character(len=len(a)+len(b))::out','integer::out').replace('out=trim(a)//trim(b)','out=a+b')
        self.reject(invalid+'use strings\nprint *,1+2\nend\n','cannot replace an intrinsic operation')


if __name__=='__main__':
    unittest.main()

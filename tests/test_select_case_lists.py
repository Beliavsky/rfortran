"""CASE pairs are selector lists, while expression pairs remain COMPLEX literals."""
import os
from pathlib import Path
import tempfile
import unittest
import test_deallocate
from test_record_read import ROOT

class SelectCaseLists(unittest.TestCase):
    def setUp(self):
        previous=Path.cwd()
        work=tempfile.TemporaryDirectory(prefix='case-list-work-')
        self.addCleanup(work.cleanup)
        self.addCleanup(os.chdir,previous)
        os.chdir(work.name)
    check=test_deallocate.Deallocate.check

    def test_named_constant_pair_fixture(self):
        self.check((ROOT/'tests/programs/xselect_case_list.f90').read_text(),
                   '0 99\n1 10\n2 10\n3 20\n4 20\n5 99')

    def test_integer_literal_pairs_signed_values_and_default(self):
        self.check("""program p
implicit none
integer::i
do i=-2,3
select case(i)
case(-1,+2)
print *,i,1
case(0,1)
print *,i,2
case default
print *,i,3
end select
end do
end
""",'-2 3\n-1 1\n0 2\n1 2\n2 1\n3 3')

    def test_unary_plus_parameter_and_case_labels(self):
        self.check("""program p
implicit none
integer,parameter::a=+1,b=+(a+1)
integer::i
do i=1,3
select case(i)
case(+a,+b)
print *,1
case default
print *,2
end select
end do
end
""",'1\n1\n2')

    def test_named_construct_and_continued_pair(self):
        self.check("""program p
implicit none
integer,parameter::a=1,b=2
integer::i
do i=1,3
choice: select case(i)
case(a, & ! keep the list distinct from a complex literal
 & b) choice
print *,10
case default choice
print *,20
end select choice
end do
end
""",'10\n10\n20')

    def test_pairs_and_unbounded_ranges(self):
        self.check("""program p
implicit none
integer::i
do i=-2,4
select case(i)
case(:-1)
print *,1
case(0,2)
print *,2
case(1)
print *,3
case(3:)
print *,4
end select
end do
end
""",'1\n1\n2\n3\n2\n4\n4')

    def test_character_pairs(self):
        self.check("""program p
implicit none
character(1)::s
integer::i
do i=1,3
s=achar(iachar('a')+i-1)
select case(s)
case('a','b')
print *,1
case default
print *,2
end select
end do
end
""",'1\n1\n2')

    def test_complex_literals_in_case_body_and_call(self):
        self.check("""program p
implicit none
integer,parameter::a=1,b=2
complex::z
integer::i
do i=1,2
select case(i)
case(a,b)
z=(a,b)
call show((3.0,-4.0))
print *,real(z),aimag(z)
end select
end do
contains
subroutine show(x)
complex,intent(in)::x
print *,real(x),aimag(x)
end subroutine
end
""",'3 -4\n1 2\n3 -4\n1 2')

    def test_select_and_case_remain_valid_identifiers(self):
        self.check("""program p
implicit none
integer::case(2),select
case=[1,2]
select=case(2)
select case(select)
case(1,2)
print *,case,select
end select
end
""",'1 2 2')

if __name__=='__main__':
    unittest.main()

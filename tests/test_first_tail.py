"""CHARACTER result specification scope, empty actuals and numeric sentinels."""
import unittest
import test_deallocate
from test_record_read import ROOT

FIRST='''pure function first(vec) result(text)
character(*),intent(in)::vec(:)
character(len=len(vec))::text
if(size(vec)>0)then
text=vec(1)
else
text='?'
end if
end function
'''

class FirstTail(unittest.TestCase):
    check=test_deallocate.Deallocate.check

    def test_empty_typed_constructor(self):
        # GNU reports zero length for this constructor at the function call.
        self.check((ROOT/'tests/programs/xfirst_character.f90').read_text(),
                   '[cat]\n[?  ]\n',reference=False)

    def test_module_name_shadowing(self):
        self.check('''module m
character(9)::vec
contains
'''+FIRST+'''end module
program main
use m,only:first
character(3)::words(2),empty(0)
words=['cat','dog']
print *,len(first(words)),first(words)
print *,len(first(empty)),'['//first(empty)//']'
end
''','3 cat\n3 [?  ]\n')

    def test_runtime_arrays(self):
        source='''integer::n
character(3)::words(3)
words=['cat','dog','fox']
read(*,*) n
print *,len(first(words(:n))),'['//first(words(:n))//']'
print *,'['//first(words(2:1))//']'
contains
'''+FIRST+'end\n'
        for n,expected in [(2,'3 [cat]\n[?  ]\n'),(0,'3 [?  ]\n[?  ]\n')]:
            with self.subTest(n=n):self.check(source,expected,data=str(n)+'\n')

    def test_sections_and_keyword(self):
        self.check('''character(3)::words(3)
words=['cat','dog','fox']
print *,first(vec=words(3:1:-1))
print *,first(words(2:1))
contains
'''+FIRST+'end\n','fox\n?\n')

    def test_numeric_sentinels_and_conversion(self):
        self.check('''double precision::empty(0),x,y
real::z
print *,first_real(empty)
x=-999
y=3
z=2
print *,x,y/2,z/4
contains
pure function first_real(vec) result(y)
double precision,intent(in)::vec(:)
double precision::y
if(size(vec)>0)then
y=vec(1)
else
y=-999
end if
end function
end
''','-999\n-999 1.5 0.5\n')

if __name__=='__main__':unittest.main(verbosity=2)

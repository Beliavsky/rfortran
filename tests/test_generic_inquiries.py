"""Generic dispatch and specialized array arguments for shape inquiries."""
import unittest
import test_deallocate
from test_record_read import ROOT

MODULE = """module inquiry_mod
interface identify
module procedure scalar_int,vector_int
end interface
contains
pure function scalar_int(x) result(y)
integer,intent(in)::x
integer::y
y=100+x
end function
pure function vector_int(x) result(y)
integer,intent(in)::x(:)
integer::y
y=10*size(x)+sum(x)
end function
end module
"""


class GenericInquiries(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_exclude_reproducer(self):
        self.check((ROOT/'xexclude.f90').read_text(encoding='utf-8'), '2 4\n')

    def test_rank_keywords_sections_and_empty_extents(self):
        self.check(MODULE+"""use inquiry_mod
real::a(2,3,4),b(0),c(5)
print *,identify(shape(a)),identify(shape(b))
print *,identify(shape(source=c(1:5:2),kind=4))
print *,identify(lbound(a)),identify(ubound(a))
print *,identify(ubound(array=a,dim=2)),identify(lbound(a,3))
print *,identify(ubound(a,kind=4)),identify(ubound(array=a,kind=4,dim=3))
end
""", '39 10\n13\n33 39\n103 101\n39 104\n')

    def test_inquiry_does_not_initialize_source(self):
        self.reject(MODULE+"""use inquiry_mod
real::a(2)
print *,identify(shape(a))
print *,a
end
""", 'uninitialized')

    def test_kind_and_rank_mismatch(self):
        self.reject(MODULE+"""use inquiry_mod
real::a(2)
print *,identify(shape(a,kind=8))
end
""", 'KIND=4')
        self.reject(MODULE.replace('scalar_int,vector_int', 'scalar_int')+"""use inquiry_mod
real::a(2)
print *,identify(shape(a))
end
""", 'no matching specific procedure')

if __name__ == '__main__':
    unittest.main(verbosity=2)

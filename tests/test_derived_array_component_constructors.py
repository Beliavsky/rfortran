"""Constructors of scalar records containing derived-type arrays."""
from pathlib import Path
import unittest
import test_deallocate as helpers

REPRO = (Path(__file__).parent/'programs/xderived_array_component_constructor.f90').read_text()

class DerivedArrayComponentConstructors(unittest.TestCase):
    check = helpers.Deallocate.check
    reject = helpers.Deallocate.reject

    def test_keyword_constructor(self):
        self.check(REPRO, "2020 2 3 4\n")

    def test_positional_constructor(self):
        self.check(REPRO.replace('frame(index=df%index,values=df%values)',
                                'frame(df%index,df%values)'), "2020 2 3 4\n")

    def test_reordered_keywords_and_sections(self):
        source = REPRO.replace('frame(index=df%index,values=df%values)',
                              'frame(values=df%values([2,1]),index=df%index([2,1]))')
        self.check(source, "2021 1 4 3\n")

    def test_empty_and_omitted_components(self):
        source = REPRO[:REPRO.index('program main')]+"""program main
use m
implicit none
type(frame)::a,b
a=frame(values=[1.,2.])
print *,allocated(a%index),size(a%values)
a%index=[date::]
b=copy(a)
print *,allocated(b%index),size(b%index),b%values
end
"""
        self.check(source, "F 2\nT 0 1 2\n")

    def test_null_component(self):
        source = REPRO[:REPRO.index('program main')]+"""program main
use m
implicit none
type(frame)::a
a=frame(index=null(),values=[1.,2.])
print *,allocated(a%index),size(a%values)
end
"""
        self.check(source, "F 2\n")

    def test_constructor_array_actual(self):
        source = REPRO.replace('frame(index=df%index,values=df%values)',
                              'frame(index=[date(2030,3),date(2031,4)],values=df%values)')
        self.check(source, "2030 4 3 4\n")

    def test_copy_independence_and_repeated_calls(self):
        source = REPRO.replace('print *,b%index(1)%year,b%index(2)%month,b%values',
            """a%index(1)%year=1999
print *,b%index(1)%year
b=frame(index=b%index([2,1]),values=b%values([2,1]))
print *,b%index(1)%year,b%index(2)%month,b%values
b=copy(a)
print *,b%index(1)%year""")
        self.check(source, "2020\n2021 1 4 3\n1999\n")

    def test_nested_allocatable_elements_rejected(self):
        self.reject("""program main
implicit none
type node
integer,allocatable::values(:)
end type
type container
type(node),allocatable::nodes(:)
end type
type(container)::a,b
allocate(a%nodes(1))
a%nodes(1)%values=[1,2]
b=container(nodes=a%nodes)
end
""", "structure constructors with derived array components whose elements own allocatable components are not yet supported")

    def test_rank_two_fixed_component(self):
        self.check("""program main
implicit none
type point
integer::i
real::x
end type
type grid
type(point)::nodes(2,2)
integer::tag=7
end type
type(point)::a(2,2)
type(grid)::g
a(1,1)=point(1,1.)
a(2,1)=point(2,2.)
a(1,2)=point(3,3.)
a(2,2)=point(4,4.)
g=grid(nodes=a)
print *,g%tag,g%nodes(2,2)%i,g%nodes(1,2)%x
end
""", "7 4 3\n")

if __name__ == '__main__':
    unittest.main()

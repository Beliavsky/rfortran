"""Owned scalar polymorphic components and their containing objects."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_deallocate
from test_record_read import ROOT,COMPILER

PREFIX="""module component_types
implicit none
type,abstract::base
contains
procedure(score_interface),deferred::score
end type
abstract interface
integer function score_interface(x) result(n)
import base
class(base),intent(in)::x
end function
end interface
type,extends(base)::first
integer::value=7
contains
procedure::score=>first_score
end type
type,extends(base)::second
integer::value=20
contains
procedure::score=>second_score
end type
type holder
class(base),allocatable::item
integer::label=9
contains
procedure::put
procedure::total
end type
contains
integer function first_score(x) result(n)
class(first),intent(in)::x
n=x%value
end function
integer function second_score(x) result(n)
class(second),intent(in)::x
n=x%value+100
end function
subroutine put(x,source)
class(holder),intent(inout)::x
class(base),intent(in)::source
if(allocated(x%item)) deallocate(x%item)
allocate(x%item,source=source)
end subroutine
integer function total(x) result(n)
class(holder),intent(in)::x
n=-1
if(allocated(x%item)) n=x%label+x%item%score()
end function
end module
"""
def program(body,decl='type(holder)::box'):
    return PREFIX+'program p\nuse component_types\nimplicit none\n'+decl+'\n'+body+'\nend\n'

class PolymorphicComponents(unittest.TestCase):
    def setUp(self):
        self.previous=Path.cwd()
        self.tmp=tempfile.TemporaryDirectory(prefix='poly-component-')
        self.work=Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)
        self.addCleanup(os.chdir,self.previous)
        os.chdir(self.work)
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_minimal_fixture(self):
        self.check((ROOT/'tests/programs/xpolymorphic_allocatable_component.f90').read_text(),'T')

    def test_typed_allocation_dispatch_select_and_deallocate(self):
        self.check(program("""print *,allocated(box%item)
allocate(first::box%item)
print *,allocated(box%item),box%item%score()
select type(v=>box%item)
type is(first)
v%value=12
class default
error stop
end select
print *,box%item%score()
deallocate(box%item)
print *,allocated(box%item)
"""),'F\nT 7\n12\nF')

    def test_source_copy_and_parent_capture(self):
        self.check(program("""allocate(box%item,source=source)
source%value=30
print *,box%label+box%item%score(),source%value
""",'type(holder)::box\ntype(first)::source'),'16 30')

    def test_owner_binding_and_class_source(self):
        self.check(program("""print *,box%total()
call box%put(a)
print *,box%total()
call box%put(b)
print *,box%total()
""",'type(holder)::box\ntype(first)::a\ntype(second)::b'),'-1\n16\n129')

    def test_two_owners_deep_copy(self):
        self.check(program("""allocate(first::a%item)
a%label=4
b=a
select type(v=>a%item)
type is(first)
v%value=30
end select
print *,a%total(),b%total()
deallocate(a%item)
print *,allocated(a%item),allocated(b%item),b%total()
""",'type(holder)::a,b'),'34 11\nF T 11')

    def test_nested_scalar_owner(self):
        self.check(program("""allocate(second::box%part%item)
print *,box%part%item%score()
deallocate(box%part%item)
print *,allocated(box%part%item)
""",'type outer\ntype(holder)::part\nend type\ntype(outer)::box'),'120\nF')

    def test_source_and_mold_from_another_component(self):
        self.check(program("""allocate(first::a%item)
select type(v=>a%item)
type is(first)
v%value=18
end select
allocate(b%item,source=a%item)
allocate(c%item,mold=a%item)
print *,a%item%score(),b%item%score(),c%item%score()
""",'type(holder)::a,b,c'),'18 18 7')

    def test_local_lifetime(self):
        self.check(PREFIX+"""program p
use component_types
call work()
call work()
contains
subroutine work()
type(holder)::box
print *,allocated(box%item)
allocate(first::box%item)
print *,box%item%score()
end subroutine
end
""",'F\n7\nF\n7')

    def test_unallocated_dispatch(self):
        self.check(program('print *,box%item%score()'),error='use of unallocated polymorphic object',reference=False)

    def test_readonly_owner_allocation_rejected(self):
        self.reject(PREFIX+"""program p
use component_types
contains
subroutine bad(x)
type(holder),intent(in)::x
allocate(first::x%item)
end subroutine
end
""",'cannot change allocation status of INTENT(IN)')

    def test_concrete_type_from_later_module(self):
        parent=PREFIX[:PREFIX.index('type,extends(base)::first')]+PREFIX[PREFIX.index('type holder'):]
        parent=parent[:parent.index('integer function first_score')]+parent[parent.index('subroutine put'):]
        child="""module implementations
use component_types,only:base
implicit none
type,extends(base)::first
integer::value=7
contains
procedure::score=>first_score
end type
contains
integer function first_score(x) result(n)
class(first),intent(in)::x
n=x%value
end function
end module
"""
        self.check(parent+child+"""program p
use component_types,only:holder
use implementations,only:first
implicit none
type(holder)::box
type(first)::a
call box%put(a)
print *,box%total()
end
""",'16')

    def test_owner_intent_out_releases_components(self):
        self.check(program("""allocate(first::box%item)
call reset(box)
print *,box%total(),box%label
contains
subroutine reset(x)
type(holder),intent(out)::x
print *,allocated(x%item)
allocate(second::x%item)
end subroutine
"""),'F\n129 9')

    def test_source_payload_with_owned_array(self):
        source=PREFIX.replace('integer::value=7','integer::value=7\ninteger,allocatable::data(:)')
        source=source.replace('n=x%value\nend function','n=x%value\nif(allocated(x%data)) n=sum(x%data)\nend function',1)
        self.check(source+"""program p
use component_types
implicit none
type(holder)::box
type(first)::a
allocate(a%data(2))
a%data=[2,3]
call box%put(a)
a%data=10
print *,box%item%score()
deallocate(box%item)
print *,sum(a%data)
end
""",'5\n20')

    def test_recursive_layout_limit_is_explicit(self):
        self.reject("""program p
implicit none
type node
class(node),allocatable::next
end type
type(node)::root
end
""",'recursive polymorphic component layouts are not yet supported')

    def test_array_component_limit_is_explicit(self):
        self.reject(PREFIX.replace('allocatable::item','allocatable::item(:)')+"""program p
use component_types
type(holder)::box
end
""",'polymorphic ALLOCATABLE component arrays are not yet supported')

    def test_module_file_round_trip(self):
        source=self.work/'component_types.f90'
        source.write_text(PREFIX)
        main=self.work/'main.f90'
        main.write_text("""program p
use component_types
type(holder)::box
type(first)::a
call box%put(a)
print *,box%total()
end
""")
        def run(args):
            result=subprocess.run(list(map(str,args)),cwd=self.work,capture_output=True,text=True,timeout=60)
            self.assertEqual(result.returncode,0,result.stderr)
            return result
        run([COMPILER,'-c',source,'-o',self.work/'types.obj'])
        run([COMPILER,'-c',main,'-o',self.work/'main.obj'])
        exe=self.work/('example.exe' if os.name=='nt' else 'example')
        run([COMPILER,'link',self.work/'main.obj',self.work/'types.obj','-o',exe])
        self.assertEqual(run([exe]).stdout.split(),['16'])

if __name__=='__main__':
    unittest.main()

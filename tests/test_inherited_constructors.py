"""Parent-component structure constructors in all scalar value contexts."""
import unittest
import subprocess
import tempfile
from pathlib import Path
import test_deallocate
from test_record_read import ROOT, COMPILER

TYPES = """module m
implicit none
integer :: calls=0
type :: base
integer :: x=1,y=2
end type
type, extends(base) :: child
integer :: z=3
end type
type, extends(child) :: grandchild
integer :: w=4
end type
contains
function make(n) result(r)
integer,intent(in)::n
type(base)::r
calls=calls+1
r=base(n,n+1)
end function
integer function total(a) result(n)
type(child),intent(in)::a
n=a%x+a%y+a%z
end function
end module
use m
implicit none
"""


class InheritedConstructors(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xinherited_constructor.f90').read_text(), '4 5 20\n4 5 20\n')

    def test_keywords_and_inherited_components(self):
        self.check(TYPES+"""type(child)::a,b,c
a=child(z=9,base=base(y=7,x=6))
b=child(z=8,x=4)
c=child()
print *,a%x,a%y,a%z,b%x,b%y,b%z,c%x,c%y,c%z
end
""", '6 7 9 4 2 8 1 2 3\n')

    def test_multilevel_and_initializers(self):
        self.check(TYPES+"""type(grandchild),parameter::p=grandchild(child(base(5,6),7),8)
type(child),save::s=child(base(10,11),12)
type(grandchild)::a
a=grandchild(w=20,child=child(z=9,base=base(7)))
print *,p%x,p%y,p%z,p%w,s%x,s%y,s%z
print *,a%x,a%y,a%z,a%w
end
""", '5 6 7 8 10 11 12\n7 2 9 20\n')

    def test_ancestor_parent_components(self):
        self.check(TYPES+"""type(grandchild)::a,b,c
a=grandchild(base=base(4,5),z=6,w=7)
b=grandchild(base(8,9),10,11)
c=grandchild(a%base,12,13)
print *,a%x,a%y,a%z,a%w,b%x,b%y,b%z,b%w,c%x,c%y,c%z,c%w
end
""", '4 5 6 7 8 9 10 11 4 5 12 13\n')

    def test_variable_parent_and_argument(self):
        self.check(TYPES+"""type(base)::b
type(child)::a
b=base(7,8)
a=child(b,9)
print *,a%x,a%y,a%z,total(child(b,10))
end
""", '7 8 9 25\n')

    def test_function_parent_evaluated_once(self):
        self.check(TYPES+"""type(child)::a
a=child(make(5),9)
print *,a%x,a%y,a%z,calls
print *,total(child(make(2),10)),calls
end
""", '5 6 9 1\n15 2\n')

    def test_array_elements_and_constructor(self):
        self.check(TYPES+"""type(child)::a(2)
a=[child(base(1,2),3),child(base(4,5),6)]
a(1)=child(make(7),9)
print *,a%x,a%y,a%z,calls
end
""", '7 4 8 5 9 6 1\n')

    def test_source_function_parent(self):
        self.check(TYPES+"""type(child),allocatable,target::a,b
allocate(a,b,source=child(make(4),20))
print *,a%x,a%y,a%z,b%x,b%y,b%z,calls
end
""", '4 5 20 4 5 20 1\n')

    def test_allocatable_parent_components(self):
        self.check("""module m
implicit none
type :: base
integer,allocatable::v(:)
end type
type,extends(base)::child
integer::n=3
end type
end module
use m
implicit none
type(base)::p
type(child)::a,b
p%v=[4,5]
a=child(p,7)
b=child(base([8,9]),10)
p%v(1)=99
print *,a%v,a%n,b%v,b%n
a=child(a%base,11)
print *,a%v,a%n
deallocate(p%v)
a=child(p,12)
print *,allocated(a%v),a%n
a=child()
print *,allocated(a%v),a%n
end
""", '4 5 7 8 9 10\n4 5 11\nF 12\nF 3\n')

    def test_loop_condition_parent_is_repeated(self):
        self.check(TYPES+"""integer::i
i=0
do while(total(child(make(i),0))<6)
i=i+1
if(i==1) cycle
end do
print *,i,calls
end
""", '3 4\n')

    def test_loop_condition_with_parent_constructor(self):
        self.check(TYPES+"""integer::i
i=0
do while(total(child(base(1,2),i))<6)
i=i+1
end do
print *,i
end
""", '3\n')

    def test_parent_and_inherited_keyword_conflict(self):
        self.reject(TYPES+'type(child)::a\na=child(base(1,2),x=4)\nend\n',
                    'specifies both parent')

    def test_wrong_parent_type(self):
        self.reject(TYPES+'type(child)::a\na=child(base=4,z=5)\nend\n',
                    'requires a scalar of type')

    def test_flat_positional_and_parent_view(self):
        self.check(TYPES+"""type(child)::a,b
type(grandchild)::c
a=child(4,5,6)
b=child(a%base,9)
c=grandchild(child=b,w=10)
print *,a%x,a%y,a%z,b%x,b%y,b%z,c%x,c%y,c%z,c%w
end
""", '4 5 6 4 5 9 4 5 9 10\n')

    def test_generic_parent_function(self):
        types=TYPES.replace('contains\nfunction make',
                            'interface build\nmodule procedure make\nend interface\ncontains\nfunction make',1)
        self.check(types+"""type(child)::a
a=child(base=build(n=6),z=20)
print *,a%x,a%y,a%z,calls
end
""", '6 7 20 1\n')

    def test_parent_array_element_evaluates_index_once(self):
        self.check(TYPES+"""type(base)::p(2)
type(child)::a
p=[base(4,5),base(6,7)]
a=child(p(index()),20)
print *,a%x,a%y,a%z,calls
contains
integer function index() result(i)
calls=calls+1
i=2
end function
end
""", '6 7 20 1\n')

    def test_source_with_owned_parent_function(self):
        types=TYPES.replace('integer :: x=1,y=2','integer :: x=1,y=2\ninteger,allocatable::v(:)')
        types=types.replace('r=base(n,n+1)','r%x=n\nr%y=n+1\nr%v=[n,n+1]')
        self.check(types+"""type(child),allocatable,target::a,b
allocate(a,b,source=child(make(4),20))
a%v(1)=99
print *,a%x,a%y,a%z,a%v,b%v,calls
end
""", '4 5 20 99 5 4 5 1\n')

    def test_pointer_parent_component_state(self):
        self.check("""module m
implicit none
type base
integer,pointer::p=>null()
end type
type,extends(base)::child
integer::n=3
end type
end module
use m
implicit none
integer,target::x
type(base)::p
type(child)::a,b
x=4
p%p=>x
a=child(p,7)
a%p=9
nullify(p%p)
b=child(p,8)
print *,x,a%p,a%n,associated(b%p),b%n
end
""", '9 9 7 F 8\n')

    def test_capture_uses_intrinsic_assignment(self):
        self.check("""module m
implicit none
integer::copies=0
type base
integer::x=1,y=2
end type
type,extends(base)::child
integer::z=3
end type
interface assignment(=)
module procedure copy_base
end interface
contains
subroutine copy_base(a,b)
type(base),intent(out)::a
type(base),intent(in)::b
copies=copies+1
a%x=-b%x
a%y=-b%y
end subroutine
function make() result(r)
type(base)::r
r%x=4
r%y=5
end function
end module
use m
implicit none
type(child)::a
a=child(make(),20)
print *,a%x,a%y,a%z,copies
end
""", '4 5 20 0\n')

    def test_standalone_saved_allocation(self):
        self.check("""module m
implicit none
type base
integer::x=1
end type
type,extends(base)::child
integer::z=2
end type
contains
subroutine step()
type(child),allocatable,target,save::a
if(.not.allocated(a)) allocate(a,source=child(base(4),20))
print *,a%x,a%z
a%x=a%x+1
end subroutine
end module
use m
implicit none
call step()
call step()
end
""", '4 20\n5 20\n')

    def test_separate_module_and_type_alias(self):
        with tempfile.TemporaryDirectory(prefix='inherited-constructors-') as tmp:
            work=Path(tmp)
            (work/'m.f90').write_text(TYPES.split('use m\n')[0])
            (work/'main.f90').write_text("""use m,only:child,parent=>base,make,total,calls
implicit none
type(child)::a
a=child(base=parent(4,5),z=20)
print *,a%x,a%y,a%z,total(child(make(6),8)),calls
end
""")
            for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                            [COMPILER,'main.f90','m.obj','-o','main.exe']):
                result=subprocess.run(command,cwd=work,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
            result=subprocess.run([work/'main.exe'],capture_output=True,text=True,timeout=15)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.split(),['4','5','20','21','1'])


if __name__ == '__main__':
    unittest.main()

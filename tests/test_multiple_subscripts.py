"""F2023 US17: multiple subscripts and multiple subscript triplets."""
import subprocess
import tempfile
import unittest
from pathlib import Path
import test_fortran2023 as support
from test_record_read import COMPILER, ROOT

class MultipleSubscripts(unittest.TestCase):
    check=support.Fortran2023.check
    reject=support.Fortran2023.reject

    def test_example(self):
        self.check((ROOT / "tests/programs/xmultiple_subscripts.f90").read_text(), " element: 32\n section: 21 23 41 43\n updated: 99\n")

    def test_elements_and_ordinary_vector_selection(self):
        self.check("""implicit none
integer::a(3,4),i,j,v(2)=[2,3]
do j=1,4
do i=1,3
a(i,j)=10*j+i
end do
end do
print *,a(@[2,3]),a(@v),a([1,3],2)
a(@v)=99
print *,a(2,3)
end
"""," 32 32 21 23\n 99\n")

    def test_mixed_subscripts_and_rank_fifteen(self):
        self.check("""implicit none
integer::a(2,2,2,2),b(1,1,1,1,1,1,1,1,1,1,1,1,1,1,1)
a=0
b=0
a(1,@[2,1],2)=7
b(@[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1])=9
print *,a(1,2,1,2),b(@[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1])
end
"""," 7 9\n")

    def test_triplets_and_broadcast(self):
        self.check("""implicit none
integer::a(4,5),i,j
do j=1,5
do i=1,4
a(i,j)=10*j+i
end do
end do
print *,a(@[1,2]:[4,5]:[2,3])
print *,a(@[1,2]:[4,5]:2)
print *,a(@[4,5]:[1,2]:-2)
print *,shape(a(@[1,2]:[4,5]))
end
"""," 21 23 51 53\n 21 23 41 43\n 54 52 34 32\n 4 4\n")

    def test_omitted_bounds_and_mixed_sections(self):
        self.check("""implicit none
integer::a(2,3,2),i,j,k,steps(2)=[1,2]
do k=1,2
do j=1,3
do i=1,2
a(i,j,k)=100*k+10*j+i
end do
end do
end do
print *,a(@:[2,3]:2,2)
print *,a(@::steps,2)
print *,a(@[2,3]:,1)
print *,a(@[2],:,@[1])
end
"""," 211 231\n 211 212 231 232\n 132\n 112 122 132\n")

    def test_writable_sections_and_argument_identity(self):
        self.check("""implicit none
integer::a(3,3),lo(2)=[1,1],hi(2)=[3,3]
a=0
a(@lo:hi:2)=7
call set(a(@[2,3]))
print *,a
contains
subroutine set(x)
integer,intent(inout)::x
x=x+9
end subroutine
end
"""," 7 0 7 0 0 0 7 9 7\n")

    def test_nondefault_lower_bounds_and_kinds(self):
        self.check("""implicit none
integer::a(-1:1,3:4)
integer(8)::coords(0:1)=[0_8,4_8]
a=0
a(@coords)=42
print *,a(0,4)
end
"""," 42\n")

    def test_builtin_and_derived_types(self):
        self.check("""implicit none
type item
integer::n
end type
type(item)::items(2,2)
character(3)::text(2,2)
logical::flags(2,2)
complex::z(2,2)
real::x(2,2)
items%n=0
text='abc'
flags=.false.
z=(1.0,2.0)
x=0.0
items(@[2,1])%n=7
text(@[2,1])='xyz'
flags(@[2,1])=.true.
z(@[2,1])=(3.0,4.0)
x(@[2,1])=5.0
print *,items(@[2,1])%n,text(@[2,1]),flags(@[2,1]),real(z(@[2,1])),x(@[2,1])
end
"""," 7 xyz T 3.0 5.0\n")

    def test_array_valued_selector_evaluated_once(self):
        self.check("""implicit none
integer::a(2,2),calls=0
a=0
a(2,1)=7
print *,a(@coords()),calls
contains
function coords() result(v)
integer::v(2)
calls=calls+1
v=[2,1]
end function
end
"""," 7 1\n")

    def test_runtime_selector_size(self):
        self.check("""implicit none
integer::v(2)=[2,1]
print *,selected(v)
contains
integer function selected(v) result(j)
integer,intent(in)::v(:)
integer::a(2,2)
a=0
a(2,1)=7
j=a(@v)
end function
end
"""," 7\n")

    def test_parameters_expressions_and_inquiries(self):
        self.check("""implicit none
integer,parameter::n=2,coords(n)=[2,1]
integer::a(n,n),v(n)=[1,0]
a=0
a(@coords)=7
print *,a(@(v+1)),a(@[integer(kind=8)::2,1]),a(@lbound(a))
end
"""," 7 7 0\n")

    def test_implied_do_output_and_constructor(self):
        self.check("""implicit none
integer::a(2,2),b(2),i
a(:,1)=[1,2]
a(:,2)=[3,4]
print *, (a(@[i,2]),i=1,2)
b=[(a(@[i,1]),i=1,2)]
print *,b
end
"""," 3 4\n 1 2\n")

    def test_lazy_conditional(self):
        self.check("""implicit none
integer::a(2,2),calls=0
a=5
print *, (.false. ? a(@coords()) : 7),calls
print *, (.true. ? a(@coords()) : a(@coords())),calls
contains
function coords() result(v)
integer::v(2)
calls=calls+1
v=[1,1]
end function
end
"""," 7 0\n 5 1\n")

    def test_loop_condition_evaluates_each_iteration(self):
        self.check("""implicit none
integer::a(2,2),i=0,calls=0
a=1
a(2,1)=0
do while(a(@coords())>0)
i=i+1
if(i==2) then
a(1,1)=0
cycle
end if
end do
print *,i,calls
contains
function coords() result(v)
integer::v(2)
calls=calls+1
v=[1,1]
end function
end
"""," 2 3\n")

    def test_triplet_expressions_evaluated_once(self):
        self.check("""implicit none
integer::a(3,3),calls=0
a=1
print *,sum(a(@[1,1]:[3,3]:step())),calls
contains
integer function step() result(s)
calls=calls+1
s=2
end function
end
"""," 4 1\n")

    def test_forwarded_writable_element_and_intent_out(self):
        self.check("""implicit none
integer::a(2,2),i=1
a=0
call outer(a(@[i,2]))
print *,a(1,2)
contains
subroutine outer(x)
integer,intent(out)::x
call inner(x)
end subroutine
subroutine inner(y)
integer,intent(out)::y
y=8
end subroutine
end
"""," 8\n")

    def test_component_array_selectors_and_sections(self):
        self.check("""implicit none
type location
integer::coords(2)
end type
type(location)::p
integer::a(2,2)
p%coords=[2,1]
a=0
a(@p%coords)=8
print *,a(@p%coords)
end
"""," 8\n")

    def test_mixed_integer_kinds_do_not_narrow_coordinates(self):
        self.check("""implicit none
integer::a(200,1)
integer(1)::v(2)=[100,-99]
a=0
a(200,1)=7
print *,a(@(v+100))
end
"""," 7\n")

    def test_integer_selector_kinds(self):
        for kind in (1,2,4,8):
            self.check(f"""implicit none
integer::a(0:1,0:1)
integer({kind})::v(2)=[1,0]
a=0
a(@v)=7
print *,a(1,0)
end
"""," 7\n")

    def test_vector_expression_as_selector(self):
        self.check("""implicit none
integer::a(2,2),v(2)=[2,1]
a=0
a(1,2)=7
print *,a(@v([2,1]))
end
"""," 7\n")

    def test_array_conditional_selector(self):
        self.check("""implicit none
integer::a(2,2),v(2)=[2,1]
integer(4)::w(2)=[1,2]
a=0
a(2,1)=7
print *,a(@(.true. ? v : w))
end
"""," 7\n")

    def test_derived_and_character_sections(self):
        self.check("""implicit none
type t
integer::n
end type
type(t)::a(2,2)
character(3)::c(2,2)
a%n=0
c='abc'
a(@[1,1]:[2,1])%n=8
c(@[1,1]:[2,1])='xyz'
print *,a(@[1,1]:[2,1])%n
print '(a)',c(@[1,1]:[2,1])
end
"""," 8 8\nxyz\nxyz\n")

    def test_plain_element_arguments_without_at(self):
        self.check("""implicit none
integer::a(2),calls=0
a=0
call set(a(index_value()))
print *,a,calls
contains
integer function index_value() result(i)
calls=calls+1
i=2
end function
subroutine set(x)
integer,intent(out)::x
x=9
end subroutine
end
"""," 0 9 1\n")

    def test_error_locations_and_bounds(self):
        self.reject('integer::a(2,2)\na=0\nprint *,a(@[0,1])\nend\n','outside')
        self.reject('integer::a(2,2)\na=0\nprint *,a(@[1,1]:[2,2]:0)\nend\n','stride')
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'invalid.f90'
            path.write_text('integer::a(2,2)\na=0\nprint *,a(@1)\nend\n')
            result=subprocess.run([COMPILER,'check',path],capture_output=True,text=True)
            self.assertIn('invalid.f90:3:1',result.stderr)

    def test_separate_compilation_and_module_constants(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory=Path(tmp)
            path=directory/'m.f90'
            path.write_text("""module m
integer,parameter::coords(2)=[2,1]
contains
integer function selected() result(j)
integer::a(2,2)
a=0
a(@coords)=7
j=a(2,1)
end function
end module
""")
            def command(*args):
                return subprocess.run([COMPILER,*map(str,args)],cwd=directory,capture_output=True,text=True,timeout=60)
            result=command('-c',path)
            self.assertEqual(result.returncode,0,result.stderr)
            path.unlink()
            main=directory/'main.f90'
            main.write_text('use m,only:selected\nprint *,selected()\nend\n')
            result=command('check',main)
            self.assertEqual(result.returncode,0,result.stderr)
            result=command('-std=f2018','check',main)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('multiple subscripts',result.stderr)

    def test_omitted_bounds_with_negative_strides(self):
        self.check("""implicit none
integer::a(2,3)
a=1
print *,shape(a(@::[-1,-1]))
print *,sum(a(@[2,3]:[1,1]:[-1,-1]))
end
"""," 0 0\n 6\n")

    def test_assumed_shape_multiple_triplets(self):
        self.check("""implicit none
integer::a(3,3),steps(2)=[2,2]
a=1
print *,total(a,steps)
contains
integer function total(a,v) result(j)
integer,intent(in)::a(:,:),v(:)
j=sum(a(@::v))
end function
end
"""," 4\n")

    def test_allocatable_and_pointer_selectors(self):
        self.check("""implicit none
integer::a(2,2)
integer,allocatable::v(:)
integer,target::t(2)=[2,1]
integer,pointer::p(:)
a=0
a(2,1)=7
allocate(v(2))
v=[2,1]
p=>t
print *,a(@v),a(@p)
end
"""," 7 7\n")

    def test_simple_procedure(self):
        self.check("""implicit none
integer::a(2,2),v(2)=[2,1]
a=0
a(2,1)=7
print *,selected(a,v)
contains
simple integer function selected(a,v) result(j)
integer,intent(in)::a(:,:),v(2)
j=a(@v)
end function
end
"""," 7\n")

    def test_runtime_size_mismatch(self):
        self.reject("""implicit none
integer::a(2,2),n=3
integer,allocatable::v(:)
a=0
allocate(v(n))
v=1
print *,a(@v)
end
""", "multiple subscript arrays must have the expected size",modes=('run',))

    def test_subscripted_parent_and_component_triplet(self):
        self.check("""implicit none
type t
integer::n(2)
end type
type(t)::a(2,2)
a%n=0
a(@[2,1])%n(@[1]:[2])=9
print *,a(@[2,1])%n(@[1]:[2])
end
"""," 9 9\n")

    def test_invalid_selectors(self):
        for source,message in [
            ('integer::a(2,2)\nprint *,a(@2)\nend','rank-one INTEGER'),
            ('integer::a(2,2)\nprint *,a(@[1.0,2.0])\nend','INTEGER'),
            ('integer::a(2,2),v(1,2)\nv=1\nprint *,a(@v)\nend','rank-one INTEGER'),
            ('integer::a(2,2)\nprint *,a(@1:2)\nend','at least one'),
            ('integer::a(2,2)\nprint *,a(@[1,1]:[2])\nend','same size'),
            ('integer::a(2,2)\nprint *,a(@[1,1,1])\nend','array rank'),
            ('integer::a(2,2)\nprint *,a(@([1,1]==[1,1]))\nend','INTEGER'),
            ('print *,abs(@[1,2])\nend','array designator'),
        ]:
            with self.subTest(source=source):self.reject(source,message)

    def test_standard_selection(self):
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'subscripts.f90'
            path.write_text('integer::a(2,2)\na=0\nprint *,a(@[1,1])\nend\n')
            for std in ('f90','f95','f2003','f2008','f2018','f2023'):
                result=subprocess.run([COMPILER,'-std='+std,'check',path],capture_output=True,text=True)
                self.assertEqual(result.returncode==0,std=='f2023',result.stderr)
                if std!='f2023':self.assertIn('multiple subscripts',result.stderr)

if __name__=='__main__':unittest.main()

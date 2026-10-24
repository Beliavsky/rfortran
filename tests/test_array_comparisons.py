"""Comparison matrix: native O0/O2, interpreter, and gfortran."""
import unittest
import test_runtime_dim_reductions
from test_record_read import ROOT

ORDERED = ("==", "/=", "<", "<=", ">", ">=", ".eq.", ".ne.", ".lt.", ".le.", ".gt.", ".ge.")
KINDS = [
    ("integer(1)", "[1,2,3]", "[3,2,1]", "[2]", ORDERED),
    ("integer(2)", "[1,2,3]", "[3,2,1]", "[2]", ORDERED),
    ("integer", "[1,2,3]", "[3,2,1]", "[2]", ORDERED),
    ("integer(8)", "[1_8,2_8,3_8]", "[3_8,2_8,1_8]", "[2_8]", ORDERED),
    ("real", "[1.,2.,3.]", "[3.,2.,1.]", "[2.]", ORDERED),
    ("double precision", "[1d0,2d0,3d0]", "[3d0,2d0,1d0]", "[2d0]", ORDERED),
    ("logical", "[.true.,.false.,.true.]", "[.false.,.false.,.true.]", "[.false.]", (".eqv.", ".neqv.")),
    ("character(len=3)", "['a  ','b  ','c  ']", "['c  ','b  ','a  ']", "['b  ']", ORDERED),
    ("complex", "[(1.,1.),(2.,0.),(3.,-1.)]", "[(3.,-1.),(2.,0.),(1.,1.)]", "[(2.,0.)]", ("==", "/=", ".eq.", ".ne.")),
    ("complex(kind=dp)", "[(1d0,1d0),(2d0,0d0),(3d0,-1d0)]", "[(3d0,-1d0),(2d0,0d0),(1d0,1d0)]", "[(2d0,0d0)]", ("==", "/=", ".eq.", ".ne.")),
]


def vector_program(decl, av, bv, sv, ops):
    lines = []
    for op in ops:
        lines += [f"mask=a {op} s(j)", "print *,mask,count(mask)",
                  f"print *,s(j) {op} a", f"print *,a {op} b",
                  f"print *,a(3:1:-1) {op} s(j)",
                  f"print *,a {op} b(j+1)",
                  f"print *,a(j) {op} s(j),all(a {op} s(j)),any(a {op} b)"]
    return f"""program main
implicit none
integer,parameter::dp=kind(0d0)
{decl},allocatable::a(:),b(:),s(:)
integer::n
n=3
allocate(a({"n" if decl.startswith(("type(","complex")) else "-1:n-2"}),b(n),s(1))
a={av}
b={bv}
s={sv}
call check(a,b,s)
contains
subroutine check(a,b,s)
{decl},intent(in)::a(:),b(:),s(:)
integer::j
logical::mask(size(a))
j=1
{chr(10).join(lines)}
end subroutine
end program
"""


class ArrayComparisons(unittest.TestCase):
    compare = test_runtime_dim_reductions.RuntimeDimReductions.compare
    check = test_runtime_dim_reductions.RuntimeDimReductions.check
    reject = test_runtime_dim_reductions.RuntimeDimReductions.reject

    def test_xacv_repro(self):
        self.check((ROOT / "tests/programs/xacv_runtime_array_scalar_comparison.f90").read_text(), "T T F\n")

    def test_builtin_vector_comparisons(self):
        for case in KINDS:
            with self.subTest(kind=case[0]):
                if case[0].startswith("character"):
                    # Keep bytewise debug-interpreter cases small while checking
                    # every operator and all scalar/array operand forms.
                    for start in range(0,len(case[4])):
                        self.compare(vector_program(*case[:4],case[4][start:start+1]))
                else:
                    self.compare(vector_program(*case))

    def test_overloaded_comparisons(self):
        module = "module comparisons_mod\nimplicit none\ntype::item\ninteger::value\nend type\n"
        definitions = []
        for i, op in enumerate(ORDERED[:6]):
            module += f"interface operator({op})\nmodule procedure cmp{i}\nend interface\n"
            definitions.append(f"""elemental logical function cmp{i}(a,b) result(y)
type(item),intent(in)::a,b
y=a%value {op} b%value
end function
""")
        module += "contains\n" + "".join(definitions) + "end module\n"
        body = vector_program("type(item)", "[item(1),item(2),item(3)]", "[item(3),item(2),item(1)]", "[item(2)]", ORDERED)
        body = body.replace("program main\nimplicit none", "program main\nuse comparisons_mod\nimplicit none")
        self.compare(module + body)

    def test_mixed_numeric_types_and_character_padding(self):
        self.compare("""program main
implicit none
integer,allocatable::a(:)
real,allocatable::s(:)
character(len=3),allocatable::text(:)
character(len=1),allocatable::short(:)
integer::n
n=3
allocate(a(n),s(1),text(n),short(1))
a=[1,2,3]
s=2.5
text=['a  ','b  ','c  ']
short='b'
call check(a,s,text,short)
contains
subroutine check(a,s,text,short)
integer,intent(in)::a(:)
real,intent(in)::s(:)
character(len=*),intent(in)::text(:),short(:)
logical::mask(size(a))
mask=a<s(1)
print *,mask,s(1)>=a,a==s(1)
mask=text==short(1)
print *,mask,text<=short(1),short(1)>=text
end subroutine
end program
""")

    def test_runtime_matrices_and_scalar_array_elements(self):
        for decl, value, op in [("integer", "2", "<="), ("double precision", "2d0", ">="),
                                ("logical", ".true.", ".eqv."), ("character(len=3)", "'a'", "<="),
                                ("complex", "(2.,1.)", "==")]:
            with self.subTest(kind=decl):
                self.compare(f"""program main
implicit none
{decl},allocatable::a(:,:),b(:,:)
integer::n
n=2
allocate(a(n,3),b(1,1))
a={value}
b={value}
call check(a,b)
contains
subroutine check(a,b)
{decl},intent(in)::a(:,:),b(:,:)
logical::mask(size(a,1),size(a,2))
mask=a {op} b(1,1)
print *,mask,count(mask,dim=1),all(mask,dim=2)
print *,b(1,1) {op} a
end subroutine
end program
""")

    def test_runtime_ranks_three_and_fifteen(self):
        for rank in (3,15):
            for decl,value,op in [("integer","2","<="),("double precision","2d0",">="),
                                  ("logical",".true.",".eqv."),("character(len=3)","'a'","=="),
                                  ("complex","(2.,1.)","/=")]:
                with self.subTest(rank=rank,kind=decl):
                    deferred=",".join([":"]*rank)
                    extents=",".join(["n"]+["1"]*(rank-2)+["3"])
                    ones=",".join(["1"]*rank)
                    mask=",".join(f"size(a,{i})" for i in range(1,rank+1))
                    self.compare(f"""program main
implicit none
{decl},allocatable::a({deferred}),b({deferred})
integer::n
n=2
allocate(a({extents}),b({ones}))
a={value}
b={value}
call check(a,b)
contains
subroutine check(a,b)
{decl},intent(in)::a({deferred}),b({deferred})
logical::mask({mask})
mask=a {op} b({ones})
print *,mask,all(mask),any(mask),count(mask)
print *,b({ones}) {op} a
end subroutine
end program
""")

    def test_extents_read_at_runtime(self):
        program="""program main
implicit none
integer,allocatable::ia(:),ib(:)
real,allocatable::ra(:),rb(:)
logical,allocatable::la(:),lb(:)
character(len=3),allocatable::ca(:),cb(:)
complex,allocatable::za(:),zb(:)
integer::n
read *,n
allocate(ia(n),ib(1),ra(n),rb(1),la(n),lb(1),ca(n),cb(1),za(n),zb(1))
ia=1
ib=2
ra=1.
rb=2.
la=.true.
lb=.false.
ca='a'
cb='b'
za=(1.,1.)
zb=(2.,0.)
call check(ia,ib,ra,rb,la,lb,ca,cb,za,zb)
contains
subroutine check(ia,ib,ra,rb,la,lb,ca,cb,za,zb)
integer,intent(in)::ia(:),ib(:)
real,intent(in)::ra(:),rb(:)
logical,intent(in)::la(:),lb(:)
character(len=*),intent(in)::ca(:),cb(:)
complex,intent(in)::za(:),zb(:)
logical::mask(size(ia))
mask=ia<ib(1)
print *,count(mask)
mask=ra<rb(1)
print *,count(mask)
mask=la.neqv.lb(1)
print *,count(mask)
mask=ca<cb(1)
print *,count(mask)
mask=za/=zb(1)
print *,count(mask)
end subroutine
end program
"""
        self.check(program,"3\n3\n3\n3\n3\n",data="3\n")
        self.check(program,"0\n0\n0\n0\n0\n",data="0\n")

    def test_integer_precision_and_complex_imaginary_components(self):
        self.check("""program main
implicit none
integer,parameter::dp=kind(0d0)
integer(8),allocatable::a(:),b(:)
complex,allocatable::z(:),s(:)
complex(kind=dp),allocatable::w(:),t(:)
allocate(a(3),b(1),z(3),s(1),w(3),t(1))
a=[9007199254740992_8,9007199254740993_8,huge(0_8)]
b=[9007199254740993_8]
z=[(2.,1.),(2.,0.),(2.,-1.)]
s=[(2.,0.)]
w=[(2d0,1d0),(2d0,0d0),(2d0,-1d0)]
t=[(2d0,0d0)]
call check(a,b,z,s,w,t)
contains
subroutine check(a,b,z,s,w,t)
integer(8),intent(in)::a(:),b(:)
complex,intent(in)::z(:),s(:)
complex(kind=dp),intent(in)::w(:),t(:)
print *,a<b(1),a==b(1),b(1)<=a
print *,z==s(1),z/=s(1)
print *,w==t(1),w/=t(1)
end subroutine
end program
""", "T F F F T F F T T\nF T F T F T\nF T F T F T\n")

    def test_section_size_does_not_read_values(self):
        self.check("""integer,allocatable::a(:)
integer::n
n=4
allocate(a(n))
print *,size(a(4:2:-1)),size(a(2:1)),size(a(:))
end
""", "3 0 4\n")

    def test_empty_inputs_and_remaining_guards(self):
        self.compare("""program main
implicit none
integer,allocatable::a(:),s(:)
allocate(a(0),s(1))
s=1
call check(a,s)
contains
subroutine check(a,s)
integer,intent(in)::a(:),s(:)
logical::mask(size(a))
mask=a<s(1)
print *,size(mask),all(mask),any(mask),count(mask)
end subroutine
end program
""")
        self.check("""program main
integer,allocatable::a(:),s(:)
allocate(a(3),s(1))
a=1
s=2
call check(a,s)
contains
subroutine check(a,s)
integer,intent(in)::a(:),s(:)
logical::mask(size(a))
mask=a<s(2)
print *,mask
end subroutine
end program
""", error="outside allocated bounds", reference=False)
        self.reject("complex::a(2),b\na=(1.,0.)\nb=(2.,0.)\nprint *,a<b\nend\n", "ordered comparisons")


if __name__ == "__main__":
    unittest.main()

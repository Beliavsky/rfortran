"""Intrinsic alternatives remain available after generic extension.

Each valid case runs in the interpreter, native -O0/-O2, and gfortran.
"""
import unittest
import test_deallocate
from test_record_read import ROOT


NAMES = ("max", "min", "minval", "maxval", "int", "dble", "sum", "sqrt", "abs")
MODULE = """module extended
implicit none
type :: token
integer :: value
end type
""" + "".join(f"interface {name}\nmodule procedure custom\nend interface\n" for name in NAMES) + """contains
integer function custom(x) result(y)
type(token),intent(in)::x
y=x%value+100
end function
end module
"""


class IntrinsicGenerics(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_xacv_repros(self):
        for name, expected in (
            ("max", "1 5 2024\n"),
            ("minval", "1 2020\n"),
            ("maxval", "5 2024\n"),
            ("int", "3 2024\n"),
        ):
            with self.subTest(intrinsic=name):
                self.check((ROOT / f"tests/programs/xacv_{name}_intrinsic.f90").read_text(encoding="utf-8"), expected)

    def test_custom_and_builtin_calls(self):
        program = """use extended
implicit none
type(token)::x
integer::a(3)
x%value=7
a=[3,1,5]
print *,max(x),min(x),minval(x),maxval(x),int(x)
print *,max(1,2,3),min(1,2,3),minval(a),maxval(a),int(3.75)
print *,sum(x),abs(x),sqrt(x),dble(x)
print *,sum(a),abs(-2),int(sqrt(4.0)),int(real(5)),int(dble(6))
end
"""
        self.check(MODULE + program, "107 107 107 107 107\n3 1 1 5 3\n107 107 107 107\n9 2 2 5 6\n")

    def test_custom_keywords_survive_constant_processing(self):
        program = "use extended\nimplicit none\ntype(token)::x\nx%value=7\n"
        program += "print *," + ",".join(f"{name}(x=x)" for name in NAMES) + "\nend\n"
        self.check(MODULE + program, " ".join(["107"] * len(NAMES)) + "\n")

    def test_elemental_arrays_and_variadic_keywords(self):
        self.check(MODULE + """use extended
implicit none
integer::a(3),b(3)
a=[3,1,5]
b=max(a2=a,a1=2,a4=4)
print *,b
print *,min(a,2),max(0,a,4),minval(a,mask=a>1)
print *,minval(a,a>1),maxval(array=a,mask=a<5)
end
""", "4 4 5\n2 1 2 4 4 5 3\n3 3\n")

    def test_nested_kind_rank_and_reordered_keywords(self):
        module = MODULE.replace("contains\n", """interface identify
module procedure scalar_default,scalar_wide,vector_default
end interface
contains
integer function scalar_default(x) result(y)
integer,intent(in)::x
y=4
end function
integer function scalar_wide(x) result(y)
integer(kind=selected_int_kind(18)),intent(in)::x
y=8
end function
integer function vector_default(x) result(y)
integer,intent(in)::x(:)
y=10+size(x)
end function
""", 1)
        self.check(module + """use extended
implicit none
integer,parameter::wide=selected_int_kind(18)
integer::a(2,2)
a=reshape([1,4,3,2],[2,2])
print *,identify(int(kind=wide,a=3.75)),identify(int(3.75))
print *,identify(max(a2=[1,2],a1=0))
print *,maxval(dim=1,array=a)
print *,identify(minval(mask=a>1,array=a))
end
""", "8 4\n12\n4 3\n4\n")

    def test_specific_takes_precedence_over_intrinsic(self):
        module = """module m
interface max
module procedure custom
end interface
contains
integer function custom(a1,a2) result(y)
integer,intent(in)::a1,a2
y=100+a1+a2
end function
end module
"""
        self.check(module + "use m\nprint *,max(1,2),max(a2=3,a1=4),max(1.0,2.0)\nend\n", "103 107 2.0\n")

    def test_invalid_type_kind_and_rank_do_not_fall_back(self):
        for declarations, expression in (
            ("", "max(.true.,.false.)"),
            ("", "max(1,2.0)"),
            ("", "max(1,2_8)"),
            ("integer::a(2),b(2,2)\na=1\nb=2", "max(a,b)"),
            ("", "minval(3)"),
            ("logical::a(2)\na=.true.", "maxval(a)"),
            ("integer::a(2)\na=1", "minval(a,mask=1)"),
            ("integer::a(2)\na=1", "maxval(a,dim=.true.)"),
            ("integer::a(2),d(1)\na=1\nd=1", "minval(a,dim=d)"),
            ("integer::a(2)\nlogical::mask(2,2)\na=1\nmask=.true.", "maxval(a,mask=mask)"),
            ("", "int(.true.)"),
            ("", "int(1.0,kind=[4])"),
        ):
            with self.subTest(expression=expression):
                self.reject(MODULE + f"use extended\n{declarations}\nprint *,{expression}\nend\n", "no matching specific procedure")

    def test_keyword_and_arity_errors(self):
        for expression, error in (
            ("max(1)", "missing required"),
            ("max(a1=1,a3=3)", "missing required"),
            ("max(a1=1,wrong=2)", "unknown keyword"),
            ("max(1,a1=2,a2=3)", "duplicate argument"),
            ("max(a1=1,2)", "positional argument after"),
            ("minval()", "missing required"),
            ("minval([1,2],wrong=1)", "unknown keyword"),
            ("int(a=1.0,wrong=4)", "unknown keyword"),
        ):
            with self.subTest(expression=expression):
                self.reject(MODULE + f"use extended\nprint *,{expression}\nend\n", error)

    def test_nonintrinsic_generic_still_rejects_unmatched(self):
        module = MODULE.replace("interface max\n", "interface choose\n", 1)
        self.reject(module + "use extended\nprint *,choose(1,2)\nend\n", "no matching specific procedure")

    def test_ambiguity_is_not_hidden_by_intrinsic(self):
        self.reject("""module m
interface max
module procedure first,second
end interface
contains
integer function first(a,b) result(y)
integer,intent(in)::a,b
y=a+b
end function
integer function second(a,b) result(y)
integer,intent(in)::a,b
y=a+b
end function
end module
use m
print *,max(1,2)
end
""", "ambiguous generic")


if __name__ == "__main__":
    unittest.main()

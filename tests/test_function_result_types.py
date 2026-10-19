"""Diagnose missing result types before function/array lowering."""
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER
import test_fortran2023 as support

SOURCE = '''module m
implicit none
real :: c=123.0
contains
simple function f(x,pow)
real,intent(in)::x(:),pow
f=sum(x**pow)
end function
end module
program main
use m
implicit none
real::x(3)=[10,20,30]
print *,f(x,1.0)
end
'''


class FunctionResultTypes(unittest.TestCase):
    check = support.Fortran2023.check

    def diagnostic(self, source, result, line, column=None):
        with tempfile.TemporaryDirectory(prefix="result-types-") as tmp:
            work=Path(tmp)
            path=work/"missing.f90"
            path.write_text(source)
            for args in ([path],["check",path],["run",path],["emit-obj",path,work/"missing.obj"]):
                with self.subTest(args=args):
                    p=subprocess.run([COMPILER,*map(str,args)],cwd=work,capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(p.returncode,0)
                    self.assertIn(f"function result `{result}` has no declared type under IMPLICIT NONE",p.stderr)
                    self.assertIn(f"missing.f90:{line}:"+(str(column) if column else ""),p.stderr)
                    self.assertNotIn("fixed constant extents",p.stderr)
                    self.assertNotIn("@deferred_extent",p.stderr)

    def test_module_implicit_none_precedes_array_extent_error(self):
        self.diagnostic(SOURCE,"f",5,17)

    def test_named_result(self):
        self.diagnostic(SOURCE.replace("function f(x,pow)","function f(x,pow) result(answer)").replace("f=sum", "answer=sum"),"answer",5)

    def test_local_implicit_none(self):
        self.diagnostic('function f()\nimplicit none\nf=1.0\nend function\n',"f",1)

    def test_multiple_sources_preserve_original_location(self):
        with tempfile.TemporaryDirectory(prefix="result types ") as tmp:
            work=Path(tmp)
            first=work/"first.f90"
            first.write_text("module first\nend module\n")
            bad=work/"missing result.f90"
            bad.write_text(SOURCE)
            p=subprocess.run([COMPILER,str(first),str(bad)],cwd=work,capture_output=True,text=True,timeout=30)
            self.assertNotEqual(p.returncode,0)
            self.assertIn("missing result.f90:5:17",p.stderr)
            self.assertIn("function result `f` has no declared type",p.stderr)
            self.assertNotIn("f90c-sources",p.stderr)

    def test_internal_function_inherits_main_implicit_none(self):
        self.diagnostic('implicit none\nprint *,f()\ncontains\nfunction f()\nf=1.0\nend function\nend\n',"f",4)

    def test_nested_function_inherits_host_implicit_none(self):
        self.diagnostic('subroutine outer()\nimplicit none\ncontains\nfunction f()\nf=1.0\nend function\nend subroutine\n',"f",4)

    def test_both_explicit_scalar_declaration_styles(self):
        for header,decl in (("simple real function f(x)",""),("simple function f(x)","real::f\n")):
            self.check(f'''module m
implicit none
contains
{header}
real,intent(in)::x
{decl}f=x+1.0
end function
end module
use m
implicit none
print *,int(f(2.0))
end
'''," 3\n")

    def test_declared_named_array_derived_and_character_results(self):
        self.check('''module m
implicit none
type box
integer::i
end type
contains
function values() result(a)
integer::a(2)
a=[3,4]
end function
function boxed() result(b)
type(box)::b
b%i=5
end function
function text() result(s)
character(2)::s
s="ok"
end function
end module
use m
implicit none
type(box)::b
b=boxed()
print *,values(),b%i
print "(a)",text()
end
'''," 3 4 5\nok\n")


if __name__=="__main__":
    unittest.main()

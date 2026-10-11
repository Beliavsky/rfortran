"""Intrinsic declarations without ::, across interpreter/native O0/O2 builds."""
from pathlib import Path
import subprocess
import tempfile
import unittest

import test_deallocate
from test_record_read import ROOT, COMPILER


class DeclarationsWithoutColons(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT / "tests/134_declarations_without_colons.f90").read_text(
            encoding="utf-8"), "8 AM 3 16 1.5 T\none 2.0 two\n")

    def test_character_entity_lengths_and_bounds(self):
        self.check("""program main
integer,parameter::n=3
character(len=n) a*5,b,words(0:1)*4
a='hello'
b='yes'
words=['four','five']
print *,len(a),a,len(b),b,len(words),words(0),lbound(words),words(1)
end program
""", "5 hello 3 yes 4 four 0 five\n")

    def test_continuations_semicolons_and_dimension_identifier(self):
        self.check("""program main
integer(kind=4) n, & ! more entities
 ! comment
 & dimension(2)
character &
 & (len=3) s; real x
n=2; dimension=[3,4]; s='yes'; x=1.5
print *,n,dimension,s,x
end
""", "2 3 4 yes 1.5\n")

    def test_typed_functions_with_local_declarations(self):
        self.check("""program main
print *,width(),twice(3)
contains
integer function width()
character(5) s
s='hello'
width=len_trim(s)
end function
pure function twice(i) result(j)
integer,intent(in)::i
integer j
j=2*i
end function
end program main
""", "5 6\n")

    def test_module_and_derived_type_components(self):
        self.check("""module m
type::point
integer x,y
end type point
contains
subroutine show()
type(point)::p
character(3) name
p%x=2
p%y=3
name='yes'
print *,p%x,p%y,name
end subroutine
end module
use m
call show()
end
""", "2 3 yes\n")

    def test_uninitialized_reads_are_still_rejected(self):
        self.reject("character(8) s\nprint *,s\nend",
                    "uninitialized variable `s`")

    def test_later_intent_does_not_apply_to_main_variable(self):
        self.check("""program main
integer n
n=7
print *,n,twice(3)
contains
pure integer function twice(i)
integer,intent(in)::i
twice=2*i
end function
end program main
""", "7 6\n")

    def test_initialization_and_attributes_require_colons(self):
        for declaration, message in [
            ("integer i=1", "declaration initialization requires `::`"),
            ("character(3) s='yes'", "declaration initialization requires `::`"),
            ("integer i(2)=[1,2]", "declaration initialization requires `::`"),
            ("integer,parameter n=3", "Parse:"),
            ("real,dimension(2) x", "Parse:"),
        ]:
            with self.subTest(declaration=declaration), tempfile.TemporaryDirectory() as tmp:
                source=Path(tmp)/"bad.f90"
                source.write_text(declaration+"\nend\n",encoding="utf-8")
                result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"bad.obj"],
                                      capture_output=True,text=True,timeout=30)
                self.assertNotEqual(result.returncode,0,result.stderr)
                self.assertIn(message,result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)

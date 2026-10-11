"""Optional unit-ending names and bare procedure END, across all backends."""
import unittest

import test_deallocate
from test_record_read import ROOT


class UnnamedProcedureEnds(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_fixture(self):
        self.check((ROOT / "tests/133_unnamed_procedure_ends.f90").read_text(
            encoding="utf-8"), "20\n")

    def test_each_subroutine_and_function_end_form(self):
        for sub_end, func_end in [("end", "end"),
                                  ("end subroutine", "end function"),
                                  ("end subroutine work", "end function twice")]:
            with self.subTest(sub_end=sub_end, func_end=func_end):
                self.check(f"""program main
integer::x
call work(x)
print *,x,twice(x)
contains
pure subroutine work(x)
integer,intent(out)::x
x=7
{sub_end}
pure integer function twice(x) result(y)
integer,intent(in)::x
y=2*x
{func_end}
end program
""", "7 14\n")

    def test_untyped_functions_with_and_without_result(self):
        self.check("""program main
print *,f(3),g(4)
contains
function f(i) result(j)
integer,intent(in)::i
integer::j
j=i+1
end function
function g(i)
integer,intent(in)::i
integer::g
g=i+2
end
end
""", "4 6\n")

    def test_unnamed_module_end_does_not_consume_assignment(self):
        self.check("""module m
integer,parameter::n=4
end module
use m
integer::x
x=n+1
print *,x
end
""", "5\n")

    def test_semicolons_continuations_and_block_ends(self):
        self.check("""program main
call work()
print *,f()
contains
subroutine work()
integer::i
do i=1,2
if (i==2) then
print *,i
end if
end do
end & ! unnamed continued end
 & subroutine; integer function f()
f=3
end &
 & function & ! named continued end
 & f
end program main
""", "2\n3\n")

    def test_external_procedures_after_unnamed_program_end(self):
        self.check("""program main
call first()
end program
subroutine first()
call second()
end subroutine
subroutine second()
print *,9
end
""", "9\n")


if __name__ == "__main__":
    unittest.main(verbosity=2)

"""Exercise the large-procedure register allocator and native object linking."""
import unittest
import test_deallocate


class LargeNativeFunctions(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_large_straight_line_procedure(self):
        # A runtime input prevents front-end constant folding. This produces
        # over 100,000 native IR instructions before Cranelift optimization,
        # exercising per-function allocator selection at O0 and O2.
        source = ("program large\nimplicit none\ninteger :: x\nread *,x\n"
                  + "x=x+1\n" * 20000 + "print *,x\nend program\n")
        self.check(source, "20007\n", data="7\n", interpreter=False,
                   compile_timeout=120)


    def test_large_called_function(self):
        source = ("program large\nimplicit none\ninteger :: x\nread *,x\n"
                  "print *, add_many(x)\ncontains\n"
                  "function add_many(x) result(y)\n"
                  "integer, intent(in) :: x\ninteger :: y\ny=x\n"
                  + "y=y+1\n" * 20000
                  + "end function\nend program\n")
        self.check(source, "20007\n", data="7\n", interpreter=False,
                   compile_timeout=120)


if __name__ == "__main__":
    unittest.main()

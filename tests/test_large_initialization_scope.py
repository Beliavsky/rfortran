"""Initialization analysis of a large scope must retain errors and guards."""
import unittest
import test_deallocate


class LargeInitializationScope(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    @staticmethod
    def source():
        declarations = "".join(f"integer :: x{i}\n" for i in range(2000))
        assignments = "".join(f"x{i}={i}\ntotal=total+x{i}\n" for i in range(2000))
        return ("program large\nimplicit none\ninteger :: n,y,total\n" + declarations
                + "read *, n\ntotal=0\n" + assignments
                + "if(n>0) y=7\nprint *,x0,x1999,total\nprint *,y\nend program\n")

    def test_large_scope_valid_and_path_dependent_reads(self):
        self.check(self.source(), "0 1999 1999000 7\n", data="1\n")
        self.check(self.source(), error="uninitialized variable `y`", data="0\n",
                   reference=False)

    def test_array_element_states_are_independent_between_branches(self):
        source = """program main
implicit none
integer :: n,a(2)
read *,n
if(n>0) then
   a(1)=7
else
   a(2)=9
end if
print *,a(1)
end program
"""
        self.check(source, "7\n", data="1\n")
        self.check(source, error="uninitialized variable `a`", data="0\n",
                   reference=False)

    def test_large_scope_definite_uninitialized_read(self):
        self.reject(self.source().replace("x1999=1999\n", ""),
                    "uninitialized variable `x1999`")


if __name__ == "__main__":
    unittest.main()

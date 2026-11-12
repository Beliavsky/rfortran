"""CHARACTER statement functions convert their actual arguments and values
to the declared lengths; actual arguments are evaluated once."""
import unittest

import test_deallocate


class CharacterStatementFunctions(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_result_and_dummy_lengths(self):
        # From gfortran.fortran-torture/execute/st_function_1.f90.
        self.check("""program main
character(8) :: foo
character(15) :: bar
character(6) :: p
character(7) :: s
foo(p) = p // "World"
bar(p) = p // "World"
s = "Hello"
print '(4a)', "[", foo("Hello"), "]"
print '(4a)', "[", bar(s), "]"
print '(4a)', "[", foo(s), "]"
end program main
""", "[Hello Wo]\n[Hello World    ]\n[Hello Wo]\n", std="legacy")

    def test_nested_definitions_substrings_and_numeric_functions(self):
        self.check("""program main
implicit none
character(3) :: up, w
character(5) :: two
integer :: n, sq, i
real :: x, h
up(w) = w(1:1) // "!"
two(w) = up(w) // up(w)
sq(n) = n*n
h(x) = x/2
print '(5a)', "[", up("abcdef"), "] [", two("xy"), "]"
print '(i0,1x,f3.1)', sq(3), h(3.0)
do i = 1, 2
   if (up("q") == "q! ") print '(a,i0)', "if ", i
end do
end program main
""", "[a! ] [x! x!]\n9 1.5\nif 1\nif 2\n", std="legacy")

    def test_pointer_dummy_and_pointer_actual(self):
        # From gfortran.fortran-torture/execute/st_function.f90: a dummy is
        # only a name, whatever attributes its declaration gives.
        self.check("""program main
call with_pointer_dummy
contains
subroutine with_pointer_dummy
character(len=4), pointer :: p, p1
character(len=4), target :: i
character(len=6) :: a
a(p) = p // '10'
p1 => i
i = '1234'
print '(a)', a(p1)
end subroutine with_pointer_dummy
end program main
""", "123410\n", std="legacy")

    def test_actual_evaluated_once(self):
        # From st_function.f90: fun2 returns the argument of its previous
        # call, so evaluating st7's actual twice changes the result.
        self.check("""integer function fun2(i)
integer i
integer, save :: val = 1
fun2 = val
val = i
end function fun2
integer function fun(i)
integer i
fun = i * 2
end function fun
program main
call multiple_eval
contains
subroutine multiple_eval
integer st7, st3, fun2, fun, i, j
st7(i) = i + fun(i)
st3(i, j) = fun(i) + fun(j)
print '(i0)', st7(fun2(10))
print '(i0)', st3(fun(2), 4)
end subroutine multiple_eval
end program main
""", "3\n16\n", std="legacy")


if __name__ == '__main__':
    unittest.main()

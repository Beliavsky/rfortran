"""Purity of internal procedures before internal-procedure lowering."""
import unittest
import test_deallocate as helpers

class InternalPurity(unittest.TestCase):
    check = helpers.Deallocate.check
    reject = helpers.Deallocate.reject

    def test_derived_result(self):
        source=(helpers.ROOT/'tests/programs/xpure_internal_derived_result.f90').read_text(encoding='utf-8')
        self.check(source,'6\n')

    def test_sibling_helpers_and_subroutine(self):
        self.check('''module m
implicit none
contains
pure integer function outer(i) result(n)
integer,intent(in)::i
call calculate(i,n)
contains
pure subroutine calculate(i,n)
integer,intent(in)::i
integer,intent(out)::n
n=twice(i)
end subroutine
pure integer function twice(i) result(j)
integer,intent(in)::i
j=2*i
end function
end function
end module
program main
use m
implicit none
print *,outer(3)
end program
''','6\n')

    def test_impure_helper_rejected(self):
        self.reject('''module m
contains
pure integer function outer(i) result(n)
integer,intent(in)::i
n=helper(i)
contains
integer function helper(i) result(j)
integer,intent(in)::i
j=i
end function
end function
end module
end
''','PURE procedure cannot call impure or unknown procedure `helper`')

    def test_pure_body_checked_inside_impure_host(self):
        self.reject('''module m
contains
subroutine outer()
contains
pure subroutine bad()
print *,1
end subroutine
end subroutine
end module
end
''','PURE procedure cannot perform external I/O')

    def test_host_array_read(self):
        self.check("""module m
contains
integer function outer(a) result(n)
integer,intent(in)::a(:)
n=at(2)
contains
pure integer function at(i) result(v)
integer,intent(in)::i
v=a(i)
end function
end function
end module
program main
use m
implicit none
print *,outer([3,7,11])
end program
""",'7\n')

    def test_host_variable_write_rejected(self):
        self.reject('''module m
contains
subroutine outer()
integer::n
contains
pure subroutine bad()
if (.true.) then
n=1
end if
end subroutine
end subroutine
end module
end
''','PURE procedure cannot modify nonlocal variable `n`')

    def test_unrelated_impure_helper_does_not_hide_pure_helper(self):
        self.check("""module m
contains
pure integer function first(i) result(n)
integer,intent(in)::i
n=helper(i)
contains
pure integer function helper(i) result(j)
integer,intent(in)::i
j=2*i
end function
end function
integer function second(i) result(n)
integer,intent(in)::i
n=helper(i)
contains
integer function helper(i) result(j)
integer,intent(in)::i
j=3*i
end function
end function
end module
program main
use m
implicit none
print *,first(2),second(2)
end program
""",'4 6\n')

    def test_same_named_helpers_do_not_leak_between_hosts(self):
        self.reject('''module m
contains
pure integer function first(i) result(n)
integer,intent(in)::i
n=helper(i)
contains
integer function helper(i) result(j)
integer,intent(in)::i
j=i
end function
end function
pure integer function second(i) result(n)
integer,intent(in)::i
n=helper(i)
contains
pure integer function helper(i) result(j)
integer,intent(in)::i
j=i
end function
end function
end module
end
''','PURE procedure cannot call impure or unknown procedure `helper`')

if __name__=='__main__':
    unittest.main()

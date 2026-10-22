"""Call-site CHARACTER length binding in local subroutine storage."""
import unittest
import test_deallocate
from test_record_read import ROOT

class DummyCharacterLengths(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_original_blocker(self):
        self.check((ROOT/'tests/154_dummy_character_lengths.f90').read_text(encoding='utf-8'),
                   '1,2,3\n4,5\n')

    def test_runtime_contents_local_arrays_and_length_arithmetic(self):
        self.check("""program main
character(4)::text
read(*,'(a)')text
call work(text)
call work('x')
call work('')
contains
subroutine work(text)
character(len=*),intent(in)::text
integer,parameter::extra=2
character(len=len(text))::copy
character(len=len(text)+extra)::wrapped
character(len=max(1,len(text)))::items(2)
copy=text
wrapped='['//copy//']'
items(1)=text
items(2)='Z'
print *,len(copy),len(wrapped),len(items)
print '(a)',wrapped
print '(a)',"'"//items(1)//"'"
end subroutine work
end program main
""", "4 6 4\n[ab  ]\n'ab  '\n1 3 1\n[x]\n'x'\n0 2 1\n[]\n' '\n",data='ab\n')

    def test_character_array_dummy_element_length(self):
        self.check("""program main
call work(['ab','cd'])
contains
subroutine work(text)
character(len=*),intent(in)::text(:)
character(len=len(text))::copy
copy=text(2)
print *,len(copy)
print '(a)',copy
end subroutine work
end program main
""", '2\ncd\n')

    def test_elemental_call_with_assumed_length_array_element(self):
        """An element of a CHARACTER(*) array dummy passed to an ELEMENTAL
        function with a CHARACTER(*) dummy, as in statistics.f90
        istat_str_check."""
        self.check('module m\nimplicit none\ncharacter(len=6), parameter :: labels(3) = ["mean  ","sd    ","median"]\ncontains\nelemental function istat_str(str) result(istat)\ncharacter(len=*), intent(in) :: str\ninteger :: istat\ndo istat=1,size(labels)\n   if (str == labels(istat)) return\nend do\nistat = 0\nend function\nfunction istat_str_check(str) result(istat)\ncharacter(len=*), intent(in) :: str(:)\ninteger :: istat(size(str))\ninteger :: i\ndo i=1,size(str)\n   istat(i) = istat_str(str(i))\n   if (istat(i) == 0) print *, "invalid: ", trim(str(i))\nend do\nend function\nend module\nuse m\ninteger :: a(3), b(2)\na = istat_str_check(["sd    ","median","max   "])\nprint *, a\nb = istat_str_check(["mean","sd  "])\nprint *, b\nend\n', " invalid: max\n 2 3 0\n 1 2\n")

    def test_nested_calls_and_block_shadowing(self):
        self.check("""program main
call outer('abc')
contains
subroutine outer(text)
character(len=*),intent(in)::text
character(len=len(text))::copy
copy=text
call inner(copy)
block
character(1)::text
character(len=len(text))::local
text='Z'
local=text
print '(a)',local
end block
end subroutine outer
subroutine inner(text)
character(len=*),intent(in)::text
character(len=len(text)+1)::copy
copy=text//'!'
print '(a)',copy
end subroutine inner
end program main
""",'abc!\nZ\n')

    def test_optional_length_dependency_is_rejected(self):
        # An OPTIONAL dummy cannot appear in this specification expression,
        # even when the actual is supplied on the particular call.
        self.reject("""program main
call work('abc')
contains
subroutine work(text)
character(len=*),intent(in),optional::text
character(len=len(text))::copy
copy=text
print '(a)',copy
end subroutine work
end program main
""",'CHARACTER length cannot depend on OPTIONAL')
        self.reject("""program main
call work()
contains
subroutine work(text)
character(len=*),intent(in),optional::text
character(len=len(text))::copy
if(present(text))copy=text
end subroutine work
end program main
""",'CHARACTER length cannot depend on OPTIONAL')

    def test_runtime_trim_length_is_not_mistaken_for_capacity(self):
        self.reject("""program main
character(4)::text
read(*,'(a)')text
call work(text)
contains
subroutine work(text)
character(len=*),intent(in)::text
character(len=len_trim(text))::copy
copy=text
print '(a)',copy
end subroutine work
end program main
""",'character constant')

if __name__ == '__main__':
    unittest.main()

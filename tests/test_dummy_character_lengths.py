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
        self.check("""program main
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
""", 'ab\n', data='ab\n')

    def test_runtime_integer_length_captured_on_entry(self):
        self.check((ROOT/'tests/programs/xautomatic_character_length.f90').read_text(encoding='utf-8'), '5\n[abcde]\n[Z    ]\n2\n[ab]\n[Z ]\n0\n[]\n[]\n0\n[]\n[]\n', data='5\n')

    def test_runtime_length_storage_limit(self):
        self.check((ROOT/'tests/programs/xautomatic_character_length.f90').read_text(encoding='utf-8'),
                   error='automatic CHARACTER length is too large', data='2147483647\n', reference=False)

    def test_runtime_block_length_and_substrings(self):
        self.check("""program main
implicit none
integer :: n, i
read *, n
do i=1,2
block
character(len=n+i) :: text
text='abcdefgh'
text(2:3)='XY'
print *,len(text)
print '(a)',text
end block
end do
end program
""", '4\naXYd\n5\naXYde\n', data='3\n')

    def test_runtime_function_local_and_nested_call(self):
        self.check("""program main
implicit none
integer :: n, value
read *, n
value=work(n)
print *, value
contains
integer function work(n) result(value)
integer,intent(in) :: n
character(len=n) :: text
text='abc'
call show(text)
value=len(text)
end function
subroutine show(text)
character(len=*),intent(in) :: text
print '(a)', '['//text//']'
end subroutine
end program
""", '[abc  ]\n5\n', data='5\n')

    def test_large_runtime_length_and_multiple_objects(self):
        self.check("""program main
use iso_fortran_env, only: int64
implicit none
integer(int64) :: n
read *,n
call work(n)
contains
subroutine work(n)
integer(int64),intent(in) :: n
character(len=max(n,1_int64)) :: text, copy
text='Z'
text(n:n)='!'
copy=text
print *,len(text),len(copy)
print '(a)',copy(1:1)//copy(n:n)
end subroutine
end program
""", '100000 100000\nZ!\n',data='100000\n')

    def test_invalid_runtime_lengths(self):
        for declaration in ('real,intent(in) :: n', 'integer,intent(in) :: n(2)'):
            actual = '2.0' if declaration.startswith('real') else '[2,3]'
            self.reject(f"""program main
call work({actual})
contains
subroutine work(n)
{declaration}
character(len=n) :: text
text='a'
print *,text
end subroutine
end program
""", 'automatic CHARACTER length must be a scalar INTEGER expression')
        self.reject("""program main
call work(3)
contains
subroutine work(n)
integer,intent(in),optional :: n
character(len=n) :: text
text='a'
print *,text
end subroutine
end program
""", 'CHARACTER length cannot depend on OPTIONAL')

    def test_runtime_length_does_not_initialize_value(self):
        self.reject("""program main
integer :: n
read *, n
call work(n)
contains
subroutine work(n)
integer,intent(in) :: n
character(len=n) :: text
print '(a)',text
end subroutine
end program
""", 'uninitialized')

if __name__ == '__main__':
    unittest.main()

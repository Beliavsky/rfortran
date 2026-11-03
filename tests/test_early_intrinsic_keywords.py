"""Keyword association when saved-object preparation folds constants early."""
import unittest
import test_deallocate
from test_record_read import ROOT


STATE = '''module state_mod
implicit none
type wrapper
real::value
end type
contains
subroutine activate()
type(wrapper),allocatable,target::object
type(wrapper),save::saved=wrapper(0.)
allocate(object)
object%value=saved%value
deallocate(object)
end subroutine
end module
'''


class EarlyIntrinsicKeywords(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_statistics_reproducer(self):
        self.check((ROOT/'tests/programs/xbin_prob_result.f90').read_text(),
                   '1.0\n0.2500 0.2500 0.5000\n')

    def test_conversion_inquiry_and_kind_keywords(self):
        self.check(STATE+'''program main
use state_mod
implicit none
integer,parameter::dp=selected_real_kind(r=307,p=15)
integer,parameter::wide=selected_int_kind(r=18)
real(kind=dp)::x
call activate()
x=3.75_dp
print *,real(kind=dp,a=7)
print *,int(kind=wide,a=x),nint(kind=wide,a=x)
print *,floor(kind=wide,a=-x),ceiling(kind=wide,a=-x)
print *,aint(kind=dp,a=x),anint(kind=dp,a=x)
print *,len(kind=wide,string='ab '),len_trim(kind=wide,string='ab ')
print *,ichar(kind=wide,c='A'),iachar(c='A',kind=wide)
print '(a)',achar(kind=1,i=65)//char(i=66,kind=1)
print *,kind(x=1.d0)
end
''', '7.0\n3 4\n-4 -3\n3.0 4.0\n3 2\n65 65\nAB\n8\n')

    def test_user_function_named_real_keeps_its_keywords(self):
        self.check(STATE+'''module user_mod
implicit none
contains
integer function real(left,right) result(value)
integer,intent(in)::left,right
value=10*left+right
end function
end module
program main
use state_mod
use user_mod
implicit none
call activate()
print *,real(right=2,left=3)
end
''', '32\n')

    def test_utility_recursive_real_conversion(self):
        self.check(STATE+'''module averages_mod
implicit none
integer,parameter::dp=selected_real_kind(15,307)
contains
recursive function moving_average_xlen(xx,xlen) result(xma)
real(kind=dp),intent(in)::xx(:),xlen
real(kind=dp)::xma(size(xx))
integer::n
n=size(xx)
if(xlen>n)then
   xma=moving_average_xlen(xx,real(n,kind=dp))
   return
end if
xma=xx
end function
end module
program main
use state_mod
use averages_mod
implicit none
call activate()
print *,moving_average_xlen([1._dp,4._dp,9._dp],5._dp)
end
''', '1.0 4.0 9.0\n')

    def test_invalid_keywords_are_diagnosed(self):
        for expression,message in [
            ('real(a=1,unknown=8)','unknown keyword'),
            ('real(1,a=2)','duplicate argument'),
            ('real(kind=8)','missing required argument'),
        ]:
            with self.subTest(expression=expression):
                self.reject(STATE+f'''program main
use state_mod
implicit none
call activate()
print *,{expression}
end
''', message)


if __name__ == '__main__':
    unittest.main()

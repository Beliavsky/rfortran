"""Forwarding array-valued function calls through assumed-shape procedures."""
from pathlib import Path
import unittest
import test_deallocate

MODULE="""module membership_forward_mod
implicit none
interface operator(.in.)
module procedure char_vec_in_vec
end interface
contains
pure function char_vec_in_vec(jvec,ivec) result(tf)
character(len=*),intent(in)::jvec(:),ivec(:)
logical::tf(size(jvec))
integer::i
forall(i=1:size(jvec)) tf(i)=any(jvec(i)==ivec)
end function char_vec_in_vec
subroutine membership(xfind,xdata,mask)
character(len=*),intent(in)::xfind(:),xdata(:)
logical,intent(out)::mask(:)
mask=xfind.in.xdata
end subroutine membership
subroutine outer(xfind,xdata,mask)
character(len=*),intent(in)::xfind(:),xdata(:)
logical,intent(out)::mask(:)
call membership(xfind,xdata,mask)
end subroutine outer
end module membership_forward_mod
"""

class ForwardArrayResults(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_repro(self):
        self.check((Path(__file__).resolve().parent/'179_character_membership_forward.f90').read_text(encoding='utf-8'),'F T F\n')

    def test_implicit_array_result_subscripts(self):
        for kind, values, expected in (
            ('integer', '1,2,3,4', '1 2 3 1'),
            ('real', '1.,2.,3.,4.', '1 2 3 1'),
            ('logical', '.true.,.false.,.false.,.false.', 'T F F T'),
            ('character(len=2)', '"ab","cd","ef","gh"', 'abcdefab'),
        ):
            with self.subTest(kind=kind):
                assignments='\n'.join(f'a({i%2+1},{i//2+1})={value}'
                                      for i,value in enumerate(values.split(',')))
                self.check(f'''program main
implicit none
{kind}::a(2,2),b(2,2)
{assignments}
call work(a,b)
print *,b
contains
subroutine work(x,y)
{kind},intent(in)::x(:,:)
{kind},intent(out)::y(:,:)
y=copy(x)
end subroutine
function copy(x)
{kind},intent(in)::x(:,:)
{kind}::copy(size(x,1),size(x,2))
copy=x
copy(2,2)=copy(1,1)
end function
end
''',expected)

    def test_multiple_forwarding_keyword_calls_and_lengths(self):
        self.check(MODULE+"""program main
use membership_forward_mod
logical::mask(3)
call outer(mask=mask,xdata=['cat ','fox '],xfind=['dog','cat','ant'])
print *,mask
call outer(['cat','dog','fox'],['dog'],mask)
print *,mask
end program main
""",'F T F\nF T F\n')

    def test_empty_search_and_empty_data(self):
        self.check(MODULE+"""program main
use membership_forward_mod
logical::empty_mask(0),mask(2)
call membership([character(len=3)::],['cat'],empty_mask)
print *,size(empty_mask)
call membership(['cat','dog'],[character(len=3)::],mask)
print *,mask
end program main
""",'0\nF F\n')

    def test_sections_and_expressions(self):
        self.check(MODULE+"""program main
use membership_forward_mod
character(len=3)::words(3)
logical::mask(3)
words=['dog','cat','ant']
call membership(words(3:1:-1),['cat','ant'],mask)
print *,mask
end program main
""",'T T F\n')

    def test_numeric_array_result_forwarding(self):
        module=MODULE.replace('character(len=*),intent(in)', 'integer,intent(in)')
        self.check(module+"""program main
use membership_forward_mod
logical::mask(3)
call outer([1,2,3],[2,4],mask)
print *,mask
end program main
""",'F T F\n')

    def test_character_array_result_forwarding(self):
        self.check("""program main
character(len=5)::out(2)
call work(['ab','cd'],out)
print '(i0,1x,a,1x,a)',len(out),trim(out(1)),trim(out(2))
contains
subroutine work(x,y)
character(len=*),intent(in)::x(:)
character(len=*),intent(out)::y(:)
y=echo(x)
end subroutine work
pure function echo(x) result(y)
character(len=*),intent(in)::x(:)
character(len=len(x))::y(size(x))
y=x
end function echo
end program main
""",'5 ab cd\n')

    def test_shape_mismatch_and_uninitialized_input(self):
        self.reject(MODULE+"""program main
use membership_forward_mod
logical::mask(2)
call membership(['dog','cat','ant'],['cat'],mask)
end program main
""",'conform')
        self.reject(MODULE+"""program main
use membership_forward_mod
character(len=3)::words(3)
logical::mask(3)
words(1)='dog'
call membership(words,['cat'],mask)
end program main
""",'uninitialized')

if __name__=='__main__':unittest.main()

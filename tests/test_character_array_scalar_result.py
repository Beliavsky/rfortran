"""Scalar numeric/LOGICAL results with read-only CHARACTER array inputs."""
import unittest
import test_deallocate
from test_record_read import ROOT

TOTAL = """function total_length(xx) result(n)
character(len=*),intent(in)::xx(:)
integer::n,i
n=0
do i=1,size(xx)
n=n+len_trim(xx(i))
end do
end function total_length
"""

class CharacterArrayScalarResult(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_original_program(self):
        self.check((ROOT/'tests/150_character_array_scalar_result.f90').read_text(encoding='utf-8'),
                   '2\n4\n')

    def test_variables_sections_empty_and_nested_results(self):
        self.check("""program main
character(3)::text(3)
integer::n
text=['ab ','c  ','def']
n=total_length(xx=text(2:3))
print *,n,total_length(text(3:1:-1))
print *,total_length([character(3)::]),total_length(['',''])
print *,total_length(wrap(['a','b']))
contains
"""+TOTAL+"""function wrap(text) result(out)
character(len=*),intent(in)::text(:)
character(len=len(text)+2)::out(size(text))
out='['//text//']'
end function wrap
end program main
""",'4 6\n0 0\n6\n')

    def test_module_typed_real_and_logical_results(self):
        self.check("""module m
contains
pure real function average_length(text) result(out)
character(len=*),intent(in)::text(:)
integer::i
out=0.0
do i=1,size(text)
out=out+real(len_trim(text(i)))
end do
out=out/real(size(text))
end function average_length
pure logical function has_match(text,target) result(out)
character(len=*),intent(in)::text(:),target
integer::i
out=.false.
do i=1,size(text)
if(text(i)==target)then
out=.true.
return
end if
end do
end function has_match
end module m
program main
use m
print *,average_length(['a ','bc'])
print *,has_match(target='b',text=['a','b'])
print *,has_match(['a','b'],'c')
end program main
""",'1.5\nT\nF\n')

    def test_runtime_contents(self):
        self.check("""program main
character(3)::text(2)
read(*,'(a)')text(1)
read(*,'(a)')text(2)
print *,total_length(text)
contains
"""+TOTAL+'end program main\n','3\n',data='ab\nc\n')

    def test_scalar_result_written_by_array_subroutine(self):
        # Match num_unique_char: SIZE-dependent local storage and an OUT result.
        self.check("""program main
print *,num_unique_char(['ab','ab','cd'])
print *,num_unique_char([character(2)::])
contains
function num_unique_char(xx) result(nq)
character(len=*),intent(in)::xx(:)
integer::nq,ipos(size(xx))
call unique_char(xx,ipos,nq)
if(nq==0)return
end function num_unique_char
subroutine unique_char(xx,ipos,nq)
character(len=*),intent(in)::xx(:)
integer,intent(out)::ipos(:),nq
integer::i,j
logical::seen
nq=0
ipos=0
if(size(xx)==0)return
do i=1,size(xx)
seen=.false.
do j=1,i-1
if(xx(i)==xx(j))seen=.true.
end do
if(.not.seen)then
nq=nq+1
ipos(nq)=i
end if
end do
end subroutine unique_char
end program main
""",'2\n0\n')

    def test_invalid_type_rank_and_initialization(self):
        for declarations,actual,message in [
            ('','[1,2]','CHARACTER actual argument requires a fixed length'),
            ('',"'ab'",'array actual'),
            ('character(1)::text(2)\n','text','uninitialized variable'),
            ("character(1)::text(2,2)\ntext='a'\n",'text','rank-one array actuals'),
        ]:
            with self.subTest(actual=actual,declarations=declarations):
                self.reject('program main\n'+declarations+'print *,total_length('+actual+')\ncontains\n'+TOTAL+'end program main\n',message)

    def test_readonly_input_is_enforced(self):
        self.reject("""program main
print *,bad(['a','b'])
contains
function bad(text) result(out)
character(len=*),intent(in)::text(:)
integer::out
text(1)='x'
out=1
end function bad
end program main
""",'INTENT(IN)')

if __name__ == '__main__':
    unittest.main()

"""Heap-backed scalar CHARACTER storage and expression lifetimes."""
import unittest
import test_deallocate
from test_record_read import ROOT


class RuntimeCharacterStorage(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xruntime_character_storage.f90').read_text(encoding='utf-8'),
                   '12000 abab\n12004  end\n6 ababab\nT 0\nF\n')

    def test_long_output(self):
        self.check("""implicit none
integer::n
read *,n
print *, '<'//repeat('ab',n)//'>'
end
""", '<'+'ab'*6000+'>\n', data='6000\n')

    def test_recursive_buffers_and_early_return(self):
        self.check("""program p
implicit none
print *, f(3),f(0)
contains
recursive function f(n) result(y)
integer,intent(in)::n
integer,allocatable::y
if(n==0) then
y=0
return
end if
y=len_trim(repeat('x',n*6000))+f(n-1)
end function
end program
""", '36000 0\n')

    def test_long_repeat_expression_and_fixed_target(self):
        self.check("""implicit none
integer::n
character(12000)::s
read *,n
s=repeat('ab',n)
print *,len_trim(s),s(1:2),s(11999:12000)
print *,len_trim(adjustl(repeat(' x',n)//'  '))
end
""", '12000 abab\n11999\n', data='6000\n')

    def test_grow_shrink_and_self_assignment(self):
        self.check("""implicit none
integer::n,i
character(:),allocatable::s
read *,n
s=repeat('ab',n)
print *,allocated(s),len(s),s(1:2),s(len(s)-1:)
s=s//trim(' cd ')
print *,len(s),s(len(s)-2:)
s=s(3:8)
print *,len(s),s
s=''
print *,allocated(s),len(s)
deallocate(s)
print *,allocated(s)
end
""", 'T 12000 abab\n12003  cd\n6 ababab\nT 0\nF\n', data='6000\n')

    def test_runtime_allocate_and_overlapping_substrings(self):
        self.check("""implicit none
integer::n
character(:),allocatable::s
read *,n
allocate(character(len=n)::s)
print *,allocated(s),len(s)
s(:)=repeat('a',n)
s(2:n)=s(1:n-1)
s(n-1:n)='bc'
print *,len(s),s(1:2),s(n-1:n)
deallocate(s)
allocate(character(len=0)::s)
print *,allocated(s),len(s)
end
""", 'T 12000\n12000 aabc\nT 0\n', data='12000\n')

    def test_reused_buffers_and_conditional_sites(self):
        self.check("""implicit none
integer::i,n
character(:),allocatable::s
read *,n
s=''
do i=1,12
if(mod(i,2)==0) then
s=trim(adjustl(' '//repeat('ab',n+i)//' '))
else
s=repeat('x',n+i)
end if
end do
print *,len(s),s(1:2),s(len(s)-1:)
end
""", '12024 abab\n', data='6000\n')

    def test_length_and_search_of_long_expressions(self):
        self.check("""implicit none
integer::n
character(:),allocatable::s
read *,n
s=repeat('x',n)//'yz'
print *,len(repeat('ab',n)),index(s,'yz'),scan(s,'z'),verify(s,'x')
end
""", '12000 6001 6002 6001\n', data='6000\n')

    def test_constant_repeat_of_runtime_string(self):
        self.check("""implicit none
integer::n
character(:),allocatable::s,t
read *,n
s=repeat('ab',n)
t=repeat(s,2)
print *,len(t),t(1:2),t(len(t)-1:)
end
""", '24000 abab\n', data='6000\n')

    def test_bounded_growth_no_longer_stops_at_4096(self):
        self.check("""implicit none
integer::n,i
character(:),allocatable::s
read *,n
s=''
do i=1,n
s=s//'x'
end do
print *,len(s),s(1:1),s(n:n)
end
""", '5000 xx\n', data='5000\n', interpreter=False)

if __name__=='__main__':
    unittest.main(verbosity=2)

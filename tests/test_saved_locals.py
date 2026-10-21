"""Persistent scalar procedure locals, including implicit SAVE and recursion."""
import unittest
import test_deallocate
from test_record_read import ROOT

class SavedLocals(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_counter_and_optional_variants(self):
        self.check((ROOT/'tests/158_saved_locals.f90').read_text(encoding='utf-8'),'1\n2\n1\n2\n')

    def test_implicit_save_and_independent_procedures(self):
        self.check("""print *,first(),second()
print *,first(),second()
contains
integer function first()
integer::n=10
n=n+1
first=n
end function first
integer function second()
integer::n=100
n=n+1
second=n
end function second
end
""",'11 101\n12 102\n')

    def test_recursive_activations_share_current_value(self):
        self.check("""print *,visit_count(2)
print *,visit_count(0)
contains
recursive function visit_count(depth) result(out)
integer,intent(in)::depth
integer,save::visits=0
integer::out
visits=visits+1
out=0
if(depth>0)out=visit_count(depth-1)
out=out+visits
end function visit_count
end
""",'9\n4\n')

    def test_numeric_kinds_and_logical_state(self):
        self.check("""call step()
call step()
contains
subroutine step()
real,save::r=0.5
double precision,save::d=1.0d0
logical,save::flag=.false.
integer(kind=8),save::wide=10000000000_8
r=r+1.0
d=d+2.0d0
flag=.not.flag
wide=wide+1_8
print *,r,d,flag,wide
end subroutine step
end
""",'1.5 3 T 10000000001\n2.5 5 F 10000000002\n')

    def test_integer_kinds_and_constant_initializers(self):
        self.check("""call step()
call step()
contains
subroutine step()
integer,parameter::base=3
integer(1),save::small=-2
integer(2)::medium=base**2
integer(4)::normal=-10
small=small+1
medium=medium+2
normal=normal+3
print *,small,medium,normal
end subroutine step
end
""",'-1 11 -7\n0 13 -4\n')

    def test_expanded_character_functions_share_one_saved_cell(self):
        self.check("""print *,number('abc')
print *,number('x')
contains
integer function number(tag)
character(len=*),intent(in)::tag
integer,save::calls=0
calls=calls+1
number=calls+len(tag)
end function number
end
""",'4\n3\n')

    def test_read_into_saved_state(self):
        self.check("""call work(.true.)
call work(.false.)
contains
subroutine work(read_value)
logical,intent(in)::read_value
integer,save::n=0
if(read_value)then
read(*,*)n
else
n=n+1
end if
print *,n
end subroutine work
end
""",'4\n5\n',data='4\n')

    def test_save_without_initializer_retains_values_and_definition_state(self):
        self.check('''call cache(.true.)
call cache(.false.)
contains
subroutine cache(set_values)
logical,intent(in) :: set_values
integer,save :: n, calls=0
real,save :: r
double precision,save :: d
logical,save :: flag
character(4),save :: text
if(set_values)then
  n=7
  r=1.5
  d=2.5d0
  flag=.true.
  text='save'
end if
calls=calls+1
print *,n,r,d,flag,text,calls
end subroutine
end
''','7 1.5 2.5 T save 1\n7 1.5 2.5 T save 2\n')

    def test_save_does_not_initialize_a_variable(self):
        self.check('''call work(.false.)
contains
subroutine work(set_value)
logical,intent(in) :: set_value
integer,save :: n
if(set_value)n=3
print *,n
end subroutine
end
''',error='uninitialized variable `n`')

    def test_cached_normal_pair_pattern(self):
        self.check('''integer :: i
do i=1,4
  print *,normal_pair()
end do
contains
function normal_pair() result(x)
double precision :: x
double precision,save :: v,sln
logical,save :: second=.false.
if(second)then
  second=.false.
  x=v*sln
else
  second=.true.
  v=3.0d0
  sln=2.0d0
  x=4.0d0
end if
end function
end
''','4\n6\n4\n6\n')

    def test_block_state_and_fresh_automatic_local(self):
        self.check("""call work()
call work()
contains
subroutine work()
integer::local
local=0
block
integer::n=0
n=n+1
local=local+1
print *,n,local
end block
end subroutine work
end
""",'1 1\n2 1\n')

    def test_saved_and_initialized_arrays(self):
        self.check("""integer::k
do k=1,3
call counts()
end do
print *,total(5),total(6)
contains
subroutine counts()
integer,save::n(3)
integer,save::calls=0
integer::c(3)=[1,2,3]
character(len=4)::s(2)=['ab','cd']
real::r(2,2)=reshape([1.0,2.0,3.0,4.0],[2,2])
logical::seen(4)
save seen
calls=calls+1
if(calls==1) then
n=0
seen=.false.
end if
n(calls)=10*calls
c=c+1
s(1)=trim(s(1))//'x'
r(2,2)=r(2,2)*2
seen(calls)=.true.
print *,n,c,s(1),r(2,2),count(seen)
end subroutine counts
integer function total(v)
integer,intent(in)::v
integer::hist(4)=0
hist(1)=hist(1)+v
total=hist(1)
end function total
end
""",'10 0 0 2 3 4 abx 8.0 1\n10 20 0 3 4 5 abxx 16.0 2\n10 20 30 4 5 6 abxx 32.0 3\n5 11\n')

    def test_saved_array_without_value_is_undefined(self):
        self.check("""call work()
contains
subroutine work()
integer,save::n(2)
print *,n(1)
end subroutine work
end
""",error='n')

    def test_invalid_or_unsupported_saved_storage(self):
        for declaration,message in [
            ('integer,save,target::n=0','TARGET persistent'),
        ]:
            with self.subTest(declaration=declaration):
                self.reject('call work()\ncontains\nsubroutine work()\n'+declaration+'\nend subroutine work\nend\n',message)
        self.reject("""print *,bad()
contains
pure integer function bad()
integer::n=0
n=n+1
bad=n
end function bad
end
""",'saved or declaration-initialized')
        self.reject('print *,bad()\ncontains\ninteger function bad()\ninteger::bad=1\nend function bad\nend\n','function result')
        self.reject('print *,bad()\ncontains\nfunction bad() result(out)\ninteger::out=1\nend function bad\nend\n','function result')

if __name__ == '__main__':
    unittest.main()

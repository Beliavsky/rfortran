"""Dynamic defined READ dispatch in interpreter and native modes."""
from pathlib import Path
import unittest
import test_deallocate

SOURCE=(Path(__file__).parent/'programs/xdefined_input_polymorphic.f90').read_text()

class PolymorphicInput(unittest.TestCase):
    check=test_deallocate.Deallocate.check

    def test_dummy_inherited_and_overridden(self):
        self.check(SOURCE,'1 2 3 4\n')

    def test_intent_out_dummy(self):
        self.check(SOURCE.replace('class(base),intent(inout) :: value','class(base),intent(out) :: value'),'1 2 3 4\n')

    def test_allocatable(self):
        self.check(SOURCE.split('program main')[0]+"""program main
use polymorphic_input
implicit none
class(base),allocatable :: a
character(6) :: text
allocate(child :: a)
text='  5  6'
read(text,'(dt)') a
select type(a)
type is(child)
 print *,a%x,a%y
end select
deallocate(a)
end program
""",'5 6\n')

    def main(self, declarations, body):
        return SOURCE.split('program main')[0]+"program main\nuse polymorphic_input\nimplicit none\n"+declarations+'\n'+body+'\nend program\n'

    def test_external_runtime_format(self):
        self.check(self.main('class(base),allocatable :: a\ninteger :: u,n,ios\ncharacter(20) :: fmt,msg', """
allocate(child :: a)
open(newunit=u,status='scratch',form='formatted')
write(u,'(a)') '  5  6  8'
rewind(u)
fmt='(dt,i3)'
msg='unchanged'
read(u,fmt,iostat=ios,iomsg=msg) a,n
close(u)
select type(a)
type is(child)
 print *,a%x,a%y,n,ios,trim(msg)
end select
deallocate(a)
"""),'5 6 8 0 unchanged\n')

    def test_list_directed_parent(self):
        self.check(self.main('class(base),allocatable :: a\ncharacter(12) :: text', """
allocate(child :: a)
text='  5  6'
read(text,*) a
select type(a)
type is(child)
 print *,a%x,a%y
end select
deallocate(a)
""").replace("read(unit,'(2i3)',", "read(unit,*,"),'5 6\n')

    def test_runtime_dynamic_type(self):
        self.check(self.main('class(base),allocatable :: a\ninteger :: i\ncharacter(6) :: text', """
text='  5  6'
do i=1,2
 if (i==1) then
  allocate(base :: a)
 else
  allocate(child :: a)
 end if
 read(text,'(dt)') a
 select type(a)
 type is(base)
  print *,a%x
 type is(child)
  print *,a%x,a%y
 end select
 deallocate(a)
end do
"""),'5\n5 6\n')

    def test_reader_error_stops_parent(self):
        source=self.main('class(base),allocatable :: a\ninteger :: ios,n\ncharacter(30) :: msg\ncharacter(9) :: text', """
allocate(child :: a)
text='  5  6  8'
n=77
read(text,'(dt,i3)',iostat=ios,iomsg=msg) a,n
print *,ios,n,trim(msg)
deallocate(a)
""")
        source=source.replace("read(unit,'(2i3)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y", "iostat=17\niomsg='child failed'")
        self.check(source,'17 77 child failed\n',reference=False)  # gfortran continues to the next item after a user error.

    def test_child_field_error(self):
        self.check(self.main('class(base),allocatable :: a\ninteger :: ios,n\ncharacter(30) :: msg\ncharacter(9) :: text', """
allocate(child :: a)
text='  5bad  8'
n=77
msg='unchanged'
read(text,'(dt,i3)',iostat=ios,iomsg=msg) a,n
print *,ios/=0,n,msg/='unchanged'
deallocate(a)
"""),'T 77 T\n')

    def test_use_only_private_readers(self):
        source=SOURCE.replace('implicit none', 'implicit none\nprivate\npublic :: base,child,inherited,fill',1)
        source=source.replace('use polymorphic_input','use polymorphic_input,only: base,child,inherited,fill')
        self.check(source,'1 2 3 4\n')

    def test_pointer_alias(self):
        self.check(self.main('type(child),target :: a\nclass(base),pointer :: p\ncharacter(6) :: text', """
p=>a
text='  5  6'
read(text,'(dt)') p
print *,a%x,a%y
nullify(p)
"""),'5 6\n')

    def test_allocatable_through_dummy(self):
        self.check(self.main('class(base),allocatable :: a', """
allocate(child :: a)
call fill(a,'  5  6')
select type(a)
type is(child)
 print *,a%x,a%y
end select
deallocate(a)
"""),'5 6\n')

if __name__=='__main__':
    unittest.main()

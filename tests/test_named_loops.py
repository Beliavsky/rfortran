"""Named DO transfers in interpreter/native O0/O2, compared with gfortran."""
import unittest
import test_deallocate


class NamedLoops(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_outer_cycle_and_exit(self):
        self.check('''implicit none
integer::i,j,k,n
n=0
do_i: do i=1,5
do j=1,3
do k=1,3
n=n+1
if(i==2) cycle do_i
if(i==4) exit do_i
exit
end do
end do
n=n+10
end do do_i
print *,i,j,k,n
end
''','4 1 1 28\n')

    def test_named_step_loops(self):
        self.check('''integer::i,j,n
n=0
outer: do i=5,1,-2
inner: do j=1,4,2
n=n+1
if(i==3) cycle outer
if(j==1) cycle inner
exit inner
end do inner
end do outer
print *,i,j,n
outer2: do i=1,6,2
do j=1,2
if(i==3) exit outer2
end do
end do outer2
print *,i,j
end
''','-1 3 5\n3 1\n')

    def test_while_and_unbounded(self):
        self.check('''integer::i,j,n
i=0;n=0
outer: do while(i<4)
i=i+1
inner: do
do j=1,3
if(i==2) cycle outer
if(i==4) exit outer
n=n+1
exit inner
end do
end do inner
end do outer
print *,i,n
end
''','4 2\n')

    def test_names_are_statement_bounded(self):
        self.check('''integer::i,value,n
n=0
value=9
outer: do i=1,3
if(i==2) cycle
value=value+1
end do outer
n=value
print *,n
again: do i=1,3; if(i==2) exit AGAIN; enddo again
print *,i
end
''','11\n2\n')

    def test_block_select_and_procedure(self):
        self.check('''integer::n
call work(n)
print *,n
contains
subroutine work(n)
integer,intent(out)::n
integer::i,j
n=0
outer: do i=1,3
do j=1,3
block
select case(i)
case(2)
cycle outer
case(3)
exit outer
case default
n=n+1
end select
end block
end do
end do outer
end subroutine
end
''','3\n')

    def test_invalid_names(self):
        for source,message in [
            ('integer::i\nouter:do i=1,2\nend do wrong\nend','does not match'),
            ('integer::i\ndo i=1,2\nend do outer\nend','requires a named DO'),
            ('integer::i\nouter:do i=1,2\nend do\nend','requires its name on END DO'),
            ('cycle missing\nend','not an enclosing DO'),
            ('integer::i,j\nouter:do i=1,2\ninner:do j=1,2\nend do inner\nexit inner\nend do outer\nend','not an enclosing DO'),
            ('integer::i,j\nouter:do i=1,2\nouter:do j=1,2\nend do outer\nend do outer\nend','duplicate DO construct'),
        ]:
            with self.subTest(source=source): self.reject(source,message)


if __name__=='__main__':
    unittest.main(verbosity=2)

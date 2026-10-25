"""Deferred CHARACTER arrays and record component argument association."""
import unittest
import test_deallocate as helpers
from test_record_read import ROOT
class DeferredCharacterComponents(unittest.TestCase):
    check=helpers.Deallocate.check
    def test_component(self):
        self.check((ROOT/'tests/programs/xdeferred_character_component.f90').read_text(),'2 3\none\ntwo\n1 6\nlonger\n')
    def test_local_allocation_and_overlap(self):
        self.check("""implicit none
character(:),allocatable::a(:)
integer::n
n=3
allocate(character(len=n)::a(2))
a='xy'
print *,len(a),size(a)
print '(a)',a
a=a//'!'
print *,len(a),size(a)
print '(a)',a
deallocate(a)
allocate(character(len=0)::a(0))
print *,len(a),size(a)
end
""",'3 2\nxy \nxy \n4 2\nxy !\nxy !\n0 0\n')
    # Bounded CHARACTER temporaries make the debug interpreter slow here.
    # Native O0/O2 and gfortran check the record-copy case.
    def test_component_copy_and_writable_dummy(self):
        self.check("""program main
implicit none
type table
character(:),allocatable::names(:)
end type
type(table)::a,b
a%names=['one','two']
b=a
call change(a)
print *,len(a%names),size(a%names),len(b%names),size(b%names)
print '(a)',a%names,b%names
contains
subroutine change(t)
type(table),intent(inout)::t
t%names=['four','five']
end subroutine
end program
""",'4 2 3 2\nfour\nfive\none\ntwo\n', interpreter=False)

    def test_structure_constructor(self):
        self.check("""program main
implicit none
type table
character(:),allocatable::names(:)
end type
type(table)::t
t=table(['abc','def'])
print *,len(t%names),size(t%names)
print '(a)',t%names
end program
""",'3 2\nabc\ndef\n')

    def test_empty_component_and_unallocated_input(self):
        self.check("""program main
implicit none
type table
character(:),allocatable::names(:)
end type
type(table)::t
call show_allocated(t)
allocate(character(len=0)::t%names(2))
t%names=''
call show_allocated(t)
print *,len(t%names),size(t%names)
deallocate(t%names)
allocate(character(len=5)::t%names(0))
call show_allocated(t)
print *,len(t%names),size(t%names)
contains
subroutine show_allocated(t)
type(table),intent(in)::t
print *,allocated(t%names)
end subroutine
end program
""",'F\nT\n0 2\nT\n5 0\n')

    def test_component_capacity_guard(self):
        self.check("""program main
implicit none
type table
character(:),allocatable::names(:)
end type
type(table)::t
integer::n
n=4097
allocate(character(len=n)::t%names(1))
end program
""",error='exceeds 4096-character temporary capacity')

    def test_allocatable_dummy(self):
        self.check("""program main
implicit none
character(:),allocatable::a(:)
call fill(a,3)
print *,len(a),size(a)
print '(a)',a
contains
subroutine fill(a,n)
character(:),allocatable,intent(out)::a(:)
integer,intent(in)::n
allocate(character(len=n)::a(2))
a='ab'
end subroutine
end program
""",'3 2\nab \nab \n')
if __name__=='__main__':unittest.main()

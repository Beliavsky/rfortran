"""CHARACTER active extents and functions exposed by descriptor specialization."""
import unittest
import test_deallocate
from test_record_read import ROOT

FIXTURE=(ROOT/'tests/194_character_active_extent.f90').read_text()
FUNCTIONS='pure function positions'+FIXTURE.split('pure function positions',1)[1].split('end program test_character_active_extent')[0]

class CharacterActiveExtent(unittest.TestCase):
    check=test_deallocate.Deallocate.check

    def test_repro(self):
        self.check(FIXTURE,'1 2 1 0\n')

    def test_deeper_nested_functions(self):
        program=FIXTURE.replace('groups=positions(words,keys)','groups=wrapped(words,keys)',1)
        wrapper='''pure function wrapped(words,keys) result(groups)
character(*),intent(in)::words(:),keys(:)
integer::groups(size(words))
groups=positions(words,keys)
end function wrapped
'''
        self.check(program.replace('pure function positions',wrapper+'pure function positions',1),'1 2 1 0\n')

    def test_different_character_lengths(self):
        self.check(FIXTURE.replace('character(4),allocatable','character(2),allocatable')
                   .replace("'red '","'rd  '").replace("'blue'","'bl  '")
                   .replace("='red'","='rd'"),'1 2 1 0\n')

    def test_repeated_empty_and_shorter_active_arrays(self):
        self.check('''program main
character(4)::words(4)
character(4),allocatable::keys(:)
integer::groups(4),n
words=['red ','blue','red ','none']
do n=2,0,-1
  call make_groups(words,keys,n,groups)
  print *,size(keys),groups
end do
contains
subroutine make_groups(words,keys,n,groups)
character(*),intent(in)::words(:)
character(*),allocatable,intent(out)::keys(:)
integer,intent(in)::n
integer,intent(out)::groups(:)
allocate(keys(n))
if(n>0) keys(1)='red'
if(n>1) keys(2)='blue'
groups=positions(words,keys)
end subroutine make_groups
'''+FUNCTIONS+'end program main\n','2 1 2 1 0\n1 1 0 1 0\n0 0 0 0 0\n')

if __name__=='__main__':unittest.main()

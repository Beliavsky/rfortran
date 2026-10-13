"""Persistent CHARACTER locals use the same storage for assignment, slices and I/O."""
import unittest
import test_deallocate

class SavedCharacters(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_persistence_padding_truncation_and_pointer_intrinsics(self):
        self.check("""call step()
call step()
contains
subroutine step()
character(4),save::s='a'
character(2)::t='long'
print *, '['//s//']','['//t//']',len(s),len_trim(s),index(s,'a')
s=trim(s)//'b'
t(2:2)='x'
end subroutine step
end
""",'[a   ][lo] 4 1 1\n[ab  ][lx] 4 2 1\n')

    def test_optional_call_variants_share_storage(self):
        self.check("""call step()
call step(.true.)
call step()
contains
subroutine step(reset)
logical,optional,intent(in)::reset
character(4)::s='x'
if(present(reset))then
if(reset)s='q'
end if
s=trim(s)//'a'
print *, '['//s//']'
end subroutine step
end
""",'[xa  ]\n[qa  ]\n[qaa ]\n')

    def test_recursive_call_shares_live_character(self):
        self.check("""call visit(2)
call visit(0)
contains
recursive subroutine visit(depth)
integer,intent(in)::depth
character(4),save::s=''
s=trim(s)//'x'
if(depth>0)call visit(depth-1)
print *, '['//s//']'
end subroutine visit
end
""",'[xxx ]\n[xxx ]\n[xxx ]\n[xxxx]\n')

    def test_read_updates_saved_buffer(self):
        self.check("""call step(.true.)
call step(.false.)
contains
subroutine step(input)
logical,intent(in)::input
character(4),save::s='init'
if(input)read(*,'(a)')s
print *, '['//s//']'
end subroutine step
end
""",'[read]\n[read]\n',data='read\n')

    def test_empty_saved_character(self):
        self.check("""call step()
call step()
contains
subroutine step()
character(0),save::s='long'
s='x'
print *, len(s),'['//s//']'
end subroutine step
end
""",'0 []\n0 []\n')

if __name__=='__main__':unittest.main()

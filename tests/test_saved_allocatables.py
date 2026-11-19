"""Saved ALLOCATABLE and POINTER locals, by SAVE or by `=> NULL()`, in the
interpreter, native O0/O2 and gfortran."""
import subprocess
import tempfile
import unittest
from pathlib import Path

import test_deallocate
from test_record_read import COMPILER


class SavedAllocatables(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_saved_arrays_keep_their_storage(self):
        # SAVE attribute, SAVE statement, and `=> NULL()`, which implies SAVE.
        self.check('''subroutine s(n)
integer, intent(in) :: n
real, allocatable, save :: a(:)
real, pointer :: p(:) => null()
real, pointer :: q(:)
save :: q
logical, save :: first = .true.
if (first) then
  nullify (q)
  first = .false.
end if
if (.not. allocated(a)) allocate (a(n))
if (.not. associated(p)) then
  allocate (p(n)); p = 0
end if
if (.not. associated(q)) then
  allocate (q(1)); q = 0
end if
a = n
p = p + n
q = q + 1
print *, size(a), size(p), sum(p), q
end subroutine
program main
call s(2)
call s(3)
end program
''', '2 2 4.0 1.0\n2 2 10.0 2.0\n')

    def test_saved_scalars_and_bare_save(self):
        self.check('''subroutine s(n)
integer, intent(in) :: n
real, pointer, save :: p
integer, allocatable, save :: k
if (.not. associated(p)) then
  allocate (p, k)
  p = 0
  k = 0
end if
p = p + n
k = k + 1
print *, p, k
end subroutine
subroutine all_saved()
save
real, allocatable :: h(:)
integer :: calls = 0
calls = calls + 1
if (.not. allocated(h)) allocate (h(0))
h = [h, real(calls)]
print *, h
end subroutine
subroutine cat(t)
character(*), intent(in) :: t
character(:), allocatable, save :: acc
if (.not. allocated(acc)) acc = ''
acc = acc // t
print *, len(acc), acc
end subroutine
program main
call s(2); call s(3)
call all_saved(); call all_saved()
call cat('ab'); call cat('cd')
end program
''', '2.0 1\n5.0 2\n1.0\n1.0 2.0\n2 ab\n4 abcd\n')

    def test_kinds_hosts_recursion_and_internal_procedures(self):
        # Kinds from a used module, the host module and the procedure; an
        # internal procedure using a saved array of its host; a recursive
        # procedure, whose activations share the saved array; a procedure of
        # the main program; the same name saved in two procedures; and
        # `=> NULL()` for a variable of the main program, of a module and of
        # a component, which SAVE does not change.
        self.check('''module kinds
implicit none
integer, parameter :: dp = kind(1.0d0)
end module kinds
module m
use kinds
implicit none
integer, parameter :: wp = dp, nc = 6
type node
  integer :: v = 0
  real, pointer :: c(:) => null()
end type
real, pointer :: g(:) => null()
contains
subroutine acc(x)
real(wp), intent(in) :: x
real(wp), allocatable, save :: hist(:)
character(len=nc), allocatable, save :: tags(:)
if (.not. allocated(hist)) allocate (hist(0), tags(0))
hist = [hist, x]
tags = [tags, 'xxxxxx']
call show()
contains
subroutine show()
print '(a,i0,a,f6.2)', 'n=', size(hist), ' sum=', sum(hist)
end subroutine
end subroutine
end module m
subroutine twod(n)
integer, intent(in) :: n
integer, parameter :: ik = selected_int_kind(9)
integer(ik), allocatable, save :: a(:,:)
integer(ik), pointer :: w(:) => null()
if (.not. allocated(a)) then
  allocate (a(2,2)); a = 0
end if
a(1,1) = a(1,1) + n
if (.not. associated(w)) then
  allocate (w(1)); w = 0
end if
w = w + 10*n
print *, sum(a), w
end subroutine
subroutine other()
real, allocatable, save :: w(:)
if (.not. allocated(w)) then
  allocate (w(3)); w = 1
end if
w = w * 2
print *, w
end subroutine
recursive subroutine rec(k)
integer, intent(in) :: k
integer, allocatable, save :: depth(:)
if (.not. allocated(depth)) allocate (depth(0))
depth = [depth, k]
if (k > 0) call rec(k - 1)
if (k == 3) print *, depth
end subroutine
program main
use m
implicit none
real, pointer :: mp(:) => null()
type(node) :: t
call acc(1.5_wp); call acc(2.25_wp)
call twod(1); call twod(2)
call other(); call other()
call rec(3)
call inner(); call inner()
print *, associated(mp), associated(g), associated(t%c), t%v
contains
subroutine inner()
integer, pointer :: c => null()
if (.not. associated(c)) then
  allocate (c); c = 0
end if
c = c + 1
print *, 'inner', c
end subroutine
end program
''', 'n=1 sum=  1.50\nn=2 sum=  3.75\n1 10\n3 30\n2.0 2.0 2.0\n4.0 4.0 4.0\n3 2 1 0\ninner 1\ninner 2\nF F F 0\n')

    def test_module_compiled_separately(self):
        # The module's object owns the storage; the program that uses the
        # module imports it.
        with tempfile.TemporaryDirectory(prefix="saved-") as tmp:
            d = Path(tmp)
            (d / "acc.f90").write_text('''MODULE Acc_Mod
implicit none
contains
subroutine add(x)
real, intent(in) :: x
real, allocatable, save :: h(:)
if (.not. allocated(h)) allocate (h(0))
h = [h, x]
print *, h
end subroutine
END MODULE Acc_Mod
''')
            (d / "main.f90").write_text("program p\nuse acc_mod\ncall add(1.0); call add(2.0)\nend program\n")
            run = lambda *args: subprocess.run([str(COMPILER), *map(str, args)], cwd=d, capture_output=True, text=True, timeout=120)
            for step in [("-c", "acc.f90", "-o", "acc.obj"), ("main.f90", "acc.obj", "-o", "main.exe")]:
                result = run(*step)
                self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([str(d / "main.exe")], capture_output=True, text=True, timeout=30)
            self.assertEqual(result.stdout.split(), "1.0 1.0 2.0".split())

    def test_block_saves_are_rejected(self):
        self.reject('''subroutine s()
block
  real, allocatable, save :: b(:)
  allocate (b(1))
end block
end subroutine
program main
call s()
end program
''', 'variable `b` of a BLOCK')
        self.reject('''subroutine s()
block
  real, pointer :: q(:) => null()
  allocate (q(1))
end block
end subroutine
program main
call s()
end program
''', 'variable `q` of a BLOCK')
        # Deferred-length CHARACTER arrays are not yet kept in module storage.
        self.reject('''subroutine s(t)
character(*), intent(in) :: t
character(:), allocatable, save :: names(:)
if (.not. allocated(names)) allocate (character(len=4) :: names(0))
names = [names, t]
end subroutine
program main
call s('abcd')
end program
''', 'persistent initialized locals currently require')


if __name__ == '__main__':
    unittest.main(verbosity=2)

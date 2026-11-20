"""An error that names a PRIVATE module entity has a note saying how to make
it PUBLIC, for every kind of entity, in single files and with modules
compiled separately, and the errors give the location of the reference.
Run after cargo build --bin rfort: python tests/test_private_entity_notes.py
"""
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER

FUNCTION_MODULE = """module m
implicit none
private
public :: g
integer :: total = 3
contains
integer function f(i)
integer, intent(in) :: i
f = 2*i
end function
integer function g()
g = 1
end function
end module m
"""

DEFAULT = ", whose entities are PRIVATE by default"

# (source, location of the reference, entity, module, PRIVATE by default)
CASES = {
    "function": (FUNCTION_MODULE + """program main
use m
implicit none
print *, f(2)
end program
""", "18:10", "f", "m", True),
    "use_only": ("""module m
implicit none
integer, private :: total = 3
end module m
program main
use m, only: total
print *, total
end program
""", "6:14", "total", "m", False),
    "renamed": ("""module m
implicit none
integer, private :: total = 3
end module m
program main
use m, t => total
implicit none
print *, t
end program
""", "6:13", "total", "m", False),
    "derived_type": ("""module m
implicit none
private
type :: point
  integer :: x
end type
public :: make
contains
function make() result(p)
type(point) :: p
p%x = 1
end function
end module m
program main
use m
type(point) :: q
q = make()
print *, q%x
end program
""", "16:6", "point", "m", True),
    "subroutine": ("""module m
implicit none
private :: show
contains
subroutine show()
print *, 'x'
end subroutine
end module m
program main
use m
call show()
end program
""", "11:6", "show", "m", False),
    "generic": ("""module m
implicit none
private :: twice
interface twice
  module procedure twice_i
end interface
contains
integer function twice_i(i)
integer, intent(in) :: i
twice_i = 2*i
end function
end module m
program main
use m
print *, twice(3)
end program
""", "15:1", "twice", "m", False),
    "kind_parameter": ("""module m
implicit none
integer, parameter, private :: wp = kind(1.0d0)
end module m
program main
use m
real(wp) :: x
x = 1
print *, x
end program
""", "7:1", "wp", "m", False),
    "operator": ("""module m
implicit none
private :: operator(.plus.)
interface operator(.plus.)
  module procedure add
end interface
contains
integer function add(a, b)
integer, intent(in) :: a, b
add = a + b
end function
end module m
program main
use m
implicit none
print *, 1 .plus. 2
end program
""", "16:1", "operator(.plus.)", "m", False),
    # Public in `n`, but `m` uses `n` and does not make it public again.
    "not_exported_again": ("""module n
implicit none
integer :: hidden = 4
end module n
module m
use n
implicit none
private
public :: k
integer :: k = 1
end module m
program main
use m
implicit none
print *, k, hidden
end program
""", "15:13", "hidden", "m", True),
}


def note(name, module, by_default):
    why = DEFAULT if by_default else ""
    return (f"help: `{name}` is PRIVATE in module `{module}`{why}; "
            f"to use it outside the module, add `public :: {name}` to module `{module}`")


class PrivateEntityNotes(unittest.TestCase):
    def invoke(self, args, directory):
        return subprocess.run([COMPILER, *map(str, args)], cwd=directory, capture_output=True,
                              text=True, encoding='utf-8', timeout=60)

    def test_each_kind_of_entity(self):
        with tempfile.TemporaryDirectory(prefix='private-notes-') as tmp:
            directory = Path(tmp)
            for case, (text, location, name, module, by_default) in CASES.items():
                source = directory / f'{case}.f90'
                source.write_text(text, encoding='utf-8')
                for args in (['check', source], [source, '-o', directory / f'{case}.exe'], ['run', source]):
                    with self.subTest(case=case, command=args[0]):
                        result = self.invoke(args, directory)
                        self.assertNotEqual(result.returncode, 0)
                        self.assertIn(f'{source.name}:{location}', result.stderr)
                        self.assertIn(note(name, module, by_default), result.stderr)
                        self.assertEqual(result.stderr.count('help:'), 1, result.stderr)

    def test_modules_compiled_separately(self):
        with tempfile.TemporaryDirectory(prefix='private-notes-') as tmp:
            directory = Path(tmp)
            (directory / 'm.f90').write_text(FUNCTION_MODULE, encoding='utf-8')
            result = self.invoke(['-c', 'm.f90', '-o', 'm.obj'], directory)
            self.assertEqual(result.returncode, 0, result.stderr)
            main = directory / 'main.f90'
            for reference, column, name in (('total', 10, 'total'), ('f(2)', 10, 'f')):
                with self.subTest(reference=reference):
                    main.write_text(f'program main\nuse m\nimplicit none\nprint *, {reference}\nend program\n',
                                    encoding='utf-8')
                    result = self.invoke(['check', main], directory)
                    self.assertNotEqual(result.returncode, 0)
                    # The reference, not the declaration in the module's source.
                    self.assertIn(f'main.f90:4:{column}', result.stderr)
                    self.assertNotIn('m.f90:', result.stderr)
                    self.assertIn(note(name, 'm', True), result.stderr)
            # When linking, an undefined function is left to the linker as an
            # external procedure. The note comes first, with a warning.
            result = self.invoke([main, 'm.obj', '-o', 'main.exe'], directory)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('warning: undefined variable `f`; it is taken to be an external procedure', result.stderr)
            self.assertIn('main.f90:4:10', result.stderr)
            self.assertIn(note('f', 'm', True), result.stderr)

    def test_no_note_for_other_names(self):
        cases = {
            # A misspelling of a local variable.
            "misspelled": """module m
implicit none
integer, private :: total = 3
end module m
program main
use m
implicit none
integer :: totl
totl = 1
print *, totel
end program
""",
            # A local variable with the name of a PRIVATE module entity.
            "own_local": """module m
implicit none
integer, private :: total
end module m
program main
use m
integer :: total
print *, total
end program
""",
            # A public entity, used wrongly.
            "public": """module m
implicit none
integer :: total = 3
end module m
program main
use m, only: totals
print *, totals
end program
""",
        }
        with tempfile.TemporaryDirectory(prefix='private-notes-') as tmp:
            directory = Path(tmp)
            for case, text in cases.items():
                with self.subTest(case=case):
                    source = directory / f'{case}.f90'
                    source.write_text(text, encoding='utf-8')
                    result = self.invoke(['check', source], directory)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertNotIn('help:', result.stderr)
                    self.assertNotIn('PRIVATE', result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)

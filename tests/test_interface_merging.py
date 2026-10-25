"""USE-associated named generics, operators and defined assignment merge."""
from pathlib import Path
import unittest
import test_deallocate as helpers

REPRO = (Path(__file__).parent / 'programs/xmerged_interfaces.f90').read_text()
EXPECTED = "12 23 31 42\n2 6 5 6\n"

class InterfaceMerging(unittest.TestCase):
    check = helpers.Deallocate.check
    reject = helpers.Deallocate.reject

    def test_local_extension_and_diamond_reexport(self):
        self.check(REPRO, EXPECTED)

    def test_independent_modules(self):
        source = REPRO.replace('module extended_interfaces\nuse base_interfaces',
                              'module extended_interfaces\nuse base_interfaces, only: box')
        self.check(source, EXPECTED)

    def test_unrestricted_import(self):
        source = REPRO.replace('use reexport_interfaces, only: box, choose, set_value, operator(-), operator(.join.), assignment(=)',
                              'use reexport_interfaces')
        self.check(source, EXPECTED)

    def test_renamed_named_generics(self):
        source = """module a
implicit none
private
public::left
interface left
module procedure specific
end interface
contains
integer function specific(x)
integer,intent(in)::x
specific=x+10
end function
end module
module b
implicit none
private
public::right
interface right
module procedure specific
end interface
contains
integer function specific(x)
real,intent(in)::x
specific=int(x)+20
end function
end module
module combined
use a,only:g=>left
use b,only:g=>right
implicit none
private
public::g
end module
program main
use combined,only:choose=>g
implicit none
print *,choose(1),choose(2.0)
end
"""
        self.check(source, "11 22\n")
        self.check(source.replace('use a,only:g=>left','use a,g=>left').replace('use b,only:g=>right','use b,g=>right'), "11 22\n")

    def test_renamed_defined_operator(self):
        source = REPRO.replace('use reexport_interfaces, only: box, choose, set_value, operator(-), operator(.join.), assignment(=)',
            'use reexport_interfaces, only: box, choose, set_value, operator(-), operator(.sum.)=>operator(.join.), assignment(=)')
        # Only the main program uses the renamed spelling.
        prefix, main = source.split('program main', 1)
        self.check(prefix+'program main'+main.replace('2 .join. 3', '2 .sum. 3').replace('2.0 .join. 4.0', '2.0 .sum. 4.0'), EXPECTED)

    def test_separately_compiled_modules(self):
        import os
        import subprocess
        import tempfile
        from test_record_read import COMPILER
        from run_ofort_compat import equivalent
        with tempfile.TemporaryDirectory(prefix='merged-modules-') as tmp:
            directory = Path(tmp)
            parts = REPRO.split('end module\n')
            objects = []
            for i, part in enumerate(parts):
                source = directory / f'unit{i}.f90'
                source.write_text(part + ('end module\n' if i < len(parts)-1 else ''))
                result = subprocess.run([COMPILER, '-c', source], cwd=directory,
                                        capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stderr)
                objects.append(source.with_suffix('.obj' if os.name == 'nt' else '.o'))
                source.unlink()
            exe = directory / ('main.exe' if os.name == 'nt' else 'main')
            result = subprocess.run([COMPILER, *objects, '-o', exe], cwd=directory,
                                    capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([exe], cwd=directory, capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(equivalent(result.stdout, EXPECTED), result.stdout)

    def test_ambiguous_named_generic(self):
        source = REPRO.replace('real,intent(in)::x\nchoose_real=int(x)+20',
                              'integer,intent(in)::x\nchoose_real=x+20')
        self.reject(source, 'ambiguous generic')

    def test_ambiguous_operator_even_when_unused(self):
        source = REPRO.replace('real function join_real(x,y)\nreal,intent(in)::x,y',
                              'integer function join_real(x,y)\ninteger,intent(in)::x,y')
        source = source[:source.index('program main')]+'program main\nuse reexport_interfaces\nimplicit none\nend\n'
        self.reject(source, 'ambiguous operator(.join.)')

    def test_ambiguous_assignment_even_when_unused(self):
        source = REPRO.replace('real,intent(in)::y\nx%value=int(y)',
                              'integer,intent(in)::y\nx%value=y')
        source = source[:source.index('program main')]+'program main\nuse reexport_interfaces\nimplicit none\nend\n'
        self.reject(source, 'ambiguous assignment(=)')

if __name__ == '__main__':
    unittest.main()

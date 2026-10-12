"""Strict LOGICAL spelling checks: equivalence must not hide dotted output."""
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from test_record_read import ROOT, build_commands

class LogicalListOutput(unittest.TestCase):
    def check(self, source, expected):
        with tempfile.TemporaryDirectory(prefix='logical-output-') as tmp:
            directory = Path(tmp)
            path = directory/'example.f90'
            path.write_text(source, encoding='utf-8')
            commands = build_commands(path, directory)
            if shutil.which('gfortran'):
                exe = directory/'reference.exe'
                built = subprocess.run(['gfortran','-std=f2018','-fcheck=all',path,'-o',exe],
                                       cwd=directory, capture_output=True, text=True, timeout=60)
                self.assertEqual(built.returncode,0,built.stderr)
                commands.append([exe])
            for command in commands:
                with self.subTest(command=command):
                    result = subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stderr,'')
                    # Ignore processor-dependent spacing, but require exact tokens.
                    self.assertEqual([' '.join(line.split()) for line in result.stdout.splitlines()],
                                     expected.splitlines())

    def test_scalars_arrays_expressions_and_character_literals(self):
        self.check((ROOT/'tests/152_logical_list_output.f90').read_text(encoding='utf-8'),
                   'F\nF\nT F\nT F\nF F T\nT F\nTF\n.TRUE..FALSE.')

    def test_runtime_values_and_function_results(self):
        self.check("""program main
logical::flags(3)
integer::i
do i=1,3
flags(i)=mod(i,2)==1
print *,flags(i),opposite(flags(i))
end do
write(*,*) (flags(i),i=1,3)
contains
pure logical function opposite(x)
logical,intent(in)::x
opposite=.not.x
end function opposite
end program main
""",'T F\nF T\nT F\nT F T')

    def test_explicit_logical_formats(self):
        self.check("""program main
logical::x,y
x=.true.
y=.false.
print '(2l1)',x,y
print '(2l3)',x,y
write(*,'(2l1)')y,x
end program main
""",'TF\nT F\nFT')

if __name__ == '__main__':
    unittest.main()

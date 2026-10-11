"""Entity CHARACTER lengths and LEN specification expressions in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import test_deallocate
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class EntityCharacterLengths(unittest.TestCase):
    def test_array_entity_lengths(self):
        for separator in ("::", ""):
            with self.subTest(separator=separator):
                test_deallocate.Deallocate.check(self,f"""program main
character {separator} a(2)*3
a=['one','two']
print *,len(a),a(1),size(a),a(2)
end program main
""","3 one 2 two\n")

    def test_execution(self):
        source=ROOT / "tests/99_entity_character_lengths.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="entity-lengths-") as tmp:
            directory=Path(tmp)
            for command in build_commands(source,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertTrue(equivalent(result.stdout,expected),result.stdout)
                    self.assertEqual(result.stderr,"")
            if shutil.which("gfortran"):
                exe=directory/"reference.exe"
                result=subprocess.run(["gfortran",source,"-o",exe],cwd=directory,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                result=subprocess.run([exe],cwd=directory,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertTrue(equivalent(expected,result.stdout),result.stdout)

    def test_host_shadowing_without_value_reads(self):
        program="""program main
character(3)::a
print *,host_length(),local_length()
contains
pure integer function host_length() result(v)
character(len=len(a)+1)::s
v=len(s)
end function host_length
pure integer function local_length() result(v)
character::a*7
character(len=len(a)+1)::s
v=len(s)
end function local_length
end program main
"""
        with tempfile.TemporaryDirectory(prefix="length-scopes-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program)
            commands=build_commands(source,directory)
            if shutil.which("gfortran"):
                exe=directory/"reference.exe"
                result=subprocess.run(["gfortran",source,"-o",exe],cwd=directory,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                commands.append([exe])
            for command in commands:
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertTrue(equivalent(result.stdout,"4 8"),result.stdout)

    def test_diagnostics(self):
        cases=[
            ("integer::i*3", "requires a CHARACTER"),
            ("integer::n\ncharacter::s*(n)", "constant INTEGER"),
            ("character::s*(*)", "assumed CHARACTER length"),
        ]
        with tempfile.TemporaryDirectory(prefix="entity-length-errors-") as tmp:
            source=Path(tmp)/"main.f90"
            for body,message in cases:
                with self.subTest(body=body):
                    source.write_text(body+"\nend\n")
                    result=subprocess.run([COMPILER,"emit-obj",source,Path(tmp)/"bad.obj"],capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

if __name__=="__main__":
    unittest.main()

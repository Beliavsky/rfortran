"""Multidimensional DIM reductions in the interpreter and native O0/O2 builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent

class DimReductions(unittest.TestCase):
    def test_execution(self):
        source=ROOT / "tests/95_dim_reductions.f90"
        expected=json.loads((ROOT/"tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        with tempfile.TemporaryDirectory(prefix="dim-reductions-") as tmp:
            directory=Path(tmp)
            for command in build_commands(source,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stdout,expected)
                    self.assertEqual(result.stderr,"")
            if shutil.which("gfortran"):
                exe=directory/"reference.exe"
                result=subprocess.run(["gfortran",source,"-o",exe],cwd=directory,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                result=subprocess.run([exe],cwd=directory,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertTrue(equivalent(expected,result.stdout),result.stdout)

    def test_shape_errors_and_masked_reads(self):
        with tempfile.TemporaryDirectory(prefix="dim-shapes-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            for body, message in [
                ("integer::a(2,3,4),b(4,2)\na=1\nb=sum(a,dim=2)", "nonconforming array assignment"),
                ("integer::a(2,3),b(3,2)\na=1\nb=1\nprint *,sum(a,dim=1,mask=b>0)", "MASK shape"),
                ("integer::a(2,3),d\na=1\nd=2\nprint *,sum(a,dim=d)", "constant INTEGER"),
            ]:
                source.write_text(body+"\nend\n")
                result=subprocess.run([COMPILER,"check",source],capture_output=True,text=True,timeout=15)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(message,result.stderr)
            source.write_text("integer::a(2,2)\nlogical::m(2,2)\na(1,1)=7\na(2,2)=9\nm=.false.\nm(1,1)=.true.\nm(2,2)=.true.\nprint *,sum(a,dim=1,mask=m)\nend\n")
            for command in build_commands(source,directory):
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(result.stdout," 7 9\n")
                self.assertEqual(result.stderr,"")

    def test_rank_three_sections_and_real(self):
        program = """program main
integer::a(2,3,4),i,j,k,denom
real::r(2,3)
double precision::d(2,3)
logical::mask(2,3,4)
do k=1,4
do j=1,3
do i=1,2
a(i,j,k)=100*k+10*j+i
end do
end do
end do
mask=a>220
r=2.0
d=3.0d0
print *,sum(a,dim=1),sum(a,dim=2),sum(a,dim=3)
print *,sum(sum(a,dim=2),dim=1)
print *,sum(a(2:1:-1,3:1:-1,4:2:-2),dim=2)
print *,sum(a,dim=3,mask=mask),count(mask,dim=2)
print *,sum(r,dim=1),product(d,dim=2)
denom=2
print *,7/denom,-7/denom,7.0/denom,a(7/denom-1,1,1)
end program main
"""
        if not shutil.which("gfortran"):
            self.skipTest("gfortran is required for this differential test")
        with tempfile.TemporaryDirectory(prefix="dim-rank-three-") as tmp:
            directory=Path(tmp)
            source=directory/"main.f90"
            source.write_text(program)
            exe=directory/"reference.exe"
            reference=subprocess.run(["gfortran",source,"-o",exe],cwd=directory,capture_output=True,text=True,timeout=60)
            self.assertEqual(reference.returncode,0,reference.stderr)
            reference=subprocess.run([exe],capture_output=True,text=True,timeout=15)
            self.assertEqual(reference.returncode,0,reference.stderr)
            for command in build_commands(source,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stderr,"")
                    self.assertTrue(equivalent(result.stdout,reference.stdout),result.stdout+"\nReference:\n"+reference.stdout)

if __name__=="__main__":
    unittest.main()

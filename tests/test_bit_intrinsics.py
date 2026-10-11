"""Scalar bit intrinsics: interpreter/native O0/O2, integer widths and bounds.

Run after cargo build --bin rfort: python tests/test_bit_intrinsics.py
RFORT_TEST_COMPILER selects another build.
"""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
from run_ofort_compat import equivalent


class BitIntrinsics(unittest.TestCase):
    def compare(self,program,expected,data=""):
        with tempfile.TemporaryDirectory(prefix="bit-intrinsics-") as tmp:
            directory=Path(tmp)
            source=directory / "bits.f90"
            source.write_text(program,encoding="utf-8")
            commands=build_commands(source,directory)
            if shutil.which("gfortran"):
                exe=directory / "reference.exe"
                result=subprocess.run(["gfortran",source,"-o",exe],cwd=directory,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                commands.append([exe])
            for command in commands:
                with self.subTest(command=command):
                    result=subprocess.run(command,cwd=directory,input=data,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertNotIn("error:",result.stderr.lower())
                    self.assertTrue(equivalent(result.stdout,expected),result.stdout)

    def test_original_family_and_contexts(self):
        source=ROOT / "tests/125_bit_intrinsics.f90"
        expected=json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        self.compare(source.read_text(encoding="utf-8"),expected)

    def test_every_integer_width_with_runtime_control_arguments(self):
        for kind in (1,2,4,8):
            width=8*kind
            sign=-(1<<(width-1))
            with self.subTest(kind=kind):
                self.compare(f"""program main
implicit none
integer({kind})::x,y
integer(8)::p,n,shift
x=-1_{kind}
y=0_{kind}
read (*,*) p
read (*,*) n
read (*,*) shift
print *,btest(x,p),ibset(y,p),ibclr(x,p)
print *,ibits(x,0,n),ibits(x,n,0),ishft(x,shift),ishft(x,-shift)
print *,iand(x,y),ior(x,y),ieor(x,y),not(x)
print *,kind(ibset(x,p)),kind(ishft(x,shift)),digits(ibclr(x,p))
end program
""",f"T {sign} {(1<<(width-1))-1}\n-1 0 0 0\n0 -1 -1 0\n{kind} {kind} {width-1}\n",
                             f"{width-1}\n{width}\n{width}\n")

    def test_nested_calls_and_condition_reevaluation(self):
        self.compare("""program main
implicit none
integer::i,value,a(3)
value=0
i=0
do while (.not. btest(value,3))
value=ibset(value,i)
i=i+1
end do
a=[1,2,3]
print *,i,value,iand(ior(value,16),not(2))
do i=1,3
a(i)=ibset(a(i),3)
end do
print *,a
end program
""","4 15 29\n9 10 11\n")

    def test_user_procedure_and_array_names(self):
        self.compare("""program main
implicit none
integer::ibset(2),value
ibset=[7,9]
value=iand(10,12)
print *,value,iand(1,2),ibset(2)
contains
integer function iand(i,j)
integer,intent(in)::i,j
iand=i+j+100
end function iand
end program
""","122 103 9\n")

    def test_bit_result_with_runtime_pack_output_in_loop(self):
        self.compare("""program main
implicit none
integer::a(3),i,value
a=[1,2,3]
value=8
do i=1,2
print *,btest(value,3),pack(a,a>i)
end do
end program
""","T 2 3\nT 3\n")

    def test_uninitialized_and_whole_array_arguments(self):
        for declarations,expression,diagnostic in [
            ("integer::i","ibset(i,0)","uninitialized variable `i`"),
            ("integer::a(3)","iand(a,1)","scalar arguments"),
            ("integer(16)::i","ibclr(i,0)","supports INTEGER kinds"),
        ]:
            with self.subTest(expression=expression),tempfile.TemporaryDirectory(prefix="unsupported-bits-") as tmp:
                directory=Path(tmp);source=directory / "invalid.f90"
                source.write_text(f"program main\nimplicit none\n{declarations}\nprint *,{expression}\nend program\n",encoding="utf-8")
                result=subprocess.run([COMPILER,source,"-o",directory / "invalid.exe"],capture_output=True,text=True,timeout=60)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(diagnostic,result.stderr)

    def test_compile_time_errors(self):
        for expression,diagnostic in [
            ("btest(0,32)","POS must be between"),
            ("ibset(0_1,8)","POS must be between"),
            ("ibclr(0,-1)","POS must be between"),
            ("ibits(0,31,2)","POS and LEN"),
            ("ibits(0,0,-1)","POS and LEN"),
            ("ishft(1,33)","SHIFT must be between"),
            ("iand(1_8,2)","same INTEGER kind"),
            ("ibits(1,0)","expects 3 arguments"),
            ("btest(1,.true.)","must be INTEGER"),
            ("not(1.0)","must be INTEGER"),
            ("iand(i=1,j=2,j=3)","duplicate"),
            ("ibset(i=1,bit=2)","unknown keyword"),
        ]:
            with self.subTest(expression=expression),tempfile.TemporaryDirectory(prefix="invalid-bits-") as tmp:
                directory=Path(tmp);source=directory / "invalid.f90"
                source.write_text(f"program main\nimplicit none\nprint *,{expression}\nend program\n",encoding="utf-8")
                for command in ([COMPILER,"run",source],[COMPILER,source,"-o",directory / "invalid.exe"]):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=60)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(diagnostic,result.stderr)

    def test_runtime_errors(self):
        for expression,data,diagnostic in [
            ("btest(i,p)","32\n0\n","BTEST: POS"),
            ("ibset(i,p)","-1\n0\n","IBSET: POS"),
            ("ibclr(i,p)","32\n0\n","IBCLR: POS"),
            ("ibits(i,p,n)","31\n2\n","IBITS: POS and LEN"),
            ("ibits(i,p,n)","0\n-1\n","IBITS: POS and LEN"),
            ("ibits(i,p,n)","-9223372036854775808\n0\n","IBITS: POS and LEN"),
            ("ibits(i,p,n)","0\n9223372036854775807\n","IBITS: POS and LEN"),
            ("ishft(i,p)","-33\n0\n","ISHFT: SHIFT"),
            ("ishft(i,p)","-9223372036854775808\n0\n","ISHFT: SHIFT"),
        ]:
            with self.subTest(expression=expression),tempfile.TemporaryDirectory(prefix="runtime-bits-") as tmp:
                directory=Path(tmp);source=directory / "bounds.f90"
                source.write_text(f"program main\nimplicit none\ninteger::i\ninteger(8)::p,n\ni=10\nread (*,*) p\nread (*,*) n\nprint *,{expression}\nend program\n",encoding="utf-8")
                for command in build_commands(source,directory):
                    result=subprocess.run(command,input=data,capture_output=True,text=True,timeout=15)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(diagnostic,result.stderr)


if __name__=="__main__":
    unittest.main(verbosity=2)

"""AT output and logical kind inquiries in interpreter and native O0/O2."""
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER, ROOT, build_commands


class Fortran2023(unittest.TestCase):
    def test_example(self):
        self.check((ROOT / "tests/programs/xfortran2023_basics.f90").read_text(),
                   "Hello from rfort\n T\n 4 4 -2\n")

    def check(self,source,expected,data=""):
        with tempfile.TemporaryDirectory(prefix="rfort-f2023-") as tmp:
            directory=Path(tmp)
            path=directory / "example.f90"
            path.write_text(source,encoding="utf-8")
            for command in build_commands(path,directory,cwd=directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,cwd=directory,input=data,capture_output=True,text=True,timeout=30)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stderr,"")
                    self.assertEqual(result.stdout,expected)

    def reject(self,source,message,modes=("check","run")):
        with tempfile.TemporaryDirectory(prefix="rfort-f2023-error-") as tmp:
            directory=Path(tmp)
            path=directory / "error.f90"
            path.write_text(source,encoding="utf-8")
            for mode in modes:
                result=subprocess.run([COMPILER,mode,path],cwd=directory,capture_output=True,text=True,timeout=30)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(message,result.stderr)

    def test_at_preserves_leading_blanks_and_trims_only_trailing_blanks(self):
        self.check('''implicit none
character(6) :: s="  abc",blank=" "
character(0) :: empty
print "(a,at,a)", "[",s,"]"
print "(a,a,a)", "[",s,"]"
print "(a,2at,a)", "[","x  "," y ","]"
print "(a,at,a)", "[",blank,"]"
print "(a,at,a)", "[",empty,"]"
end
''', "[  abc]\n[  abc ]\n[x y]\n[]\n[]\n")

    def test_at_arrays_reversion_runtime_formats_and_internal_write(self):
        self.check('''implicit none
character(4) :: a(3)=["ab  "," c  ","    "]
character(20) :: fmt="(at)",buffer
print "(*(at,':'))",a
print "(at)",a
print fmt,a(2)
write(buffer,"(at)") a(1)
print "(a,at,a)","[",buffer,"]"
write(*,"(at)",advance="no") "x  "
write(*,"(at)") "y "
end
''', "ab: c::\nab\n c\n\n c\n[ab]\nxy\n")

    def test_at_rejects_width_precision_and_input(self):
        for fmt in ("(at3)","(at0)","(at.2)"):
            with self.subTest(fmt=fmt):
                self.reject(f'print "{fmt}","abc"\nend\n',
                            "cannot have a width" if "." not in fmt else "invalid output descriptor precision")
        self.reject('character(3)::s\nread(*,"(at)") s\nend\n',"only permitted for output")
        self.reject('print "(at)",3\nend\n',"descriptor",modes=("run",))

    def test_selected_logical_kind_constants_and_storage_metadata(self):
        self.check('''use iso_fortran_env,only:logical8,logical16,logical32,logical64,real16,logical_kinds
implicit none
integer,parameter :: lk=selected_logical_kind(bits=32)
logical(kind=lk) :: a=.true.,b(2)
b=.false.
print *,logical8,logical16,logical32,logical64,real16
print *,logical_kinds,lk,kind(a)
print *,selected_logical_kind(1),selected_logical_kind(32),selected_logical_kind(33)
print *,a,b
end
''', " -2 -2 4 -1 -2\n 4 4 4\n 4 4 -1\n T F F\n")

    def test_selected_logical_kind_runtime_and_keyword_bits(self):
        self.check('''implicit none
integer :: n
integer(8) :: large
read(*,*) n,large
print *,selected_logical_kind(bits=n),selected_logical_kind(large)
end
''', " 4 -1\n",data="32 33\n")

    def test_logical_kind_in_derived_components_and_pure_inquiries(self):
        self.check('''module m
implicit none
integer,parameter :: lk=selected_logical_kind(32)
type t
logical(lk) :: flag
end type
contains
pure integer function choose(n) result(k)
integer,intent(in)::n
k=selected_logical_kind(bits=n)
end function
end module
program p
use m
implicit none
type(t) :: item,objects(2)
item%flag=.true.
objects(1)%flag=.false.
objects(2)%flag=.true.
print *,item%flag,objects(1)%flag,objects(2)%flag,choose(1),choose(33)
end program
'''," T F T 4 -1\n")

    def test_user_defined_selected_logical_kind_is_preserved(self):
        self.check('''module m
contains
integer function selected_logical_kind(bits) result(k)
integer,intent(in)::bits
k=bits+100
end function
end module
program p
use m
print *,selected_logical_kind(1)
end program
'''," 101\n")

    def test_logical_kind_direct_expression_and_procedure_result(self):
        self.check('''module m
implicit none
contains
pure logical(kind=selected_logical_kind(32)) function positive(i) result(ok)
integer,intent(in)::i
logical(kind=4)::other
other=i>0
ok=other
end function
end module
program p
use m
implicit none
logical(selected_logical_kind(1))::x
x=positive(1)
print *,x,positive(-1)
end program
'''," T F\n")

    def test_selected_logical_kind_invalid_arguments_and_storage_kinds(self):
        for expr in ("1.0",".true.","[1,2]"):
            with self.subTest(expr=expr):
                self.reject(f"print *,selected_logical_kind({expr})\nend\n","INTEGER")
        self.reject("logical(8)::x\nend\n","unsupported LOGICAL kind 8")
        self.reject("print *,selected_logical_kind()\nend\n","argument")
        self.reject("print *,selected_logical_kind(size=32)\nend\n","unknown keyword")


if __name__=="__main__": unittest.main(verbosity=2)

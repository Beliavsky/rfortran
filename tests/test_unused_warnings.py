"""Unused declaration diagnostics and CLI controls, before source lowering.
Run after cargo build --bin rfort: python tests/test_unused_warnings.py
"""
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER,ROOT

class UnusedWarnings(unittest.TestCase):
    def check(self,source,flags=(),errors=False):
        with tempfile.TemporaryDirectory(prefix="unused-warnings-") as tmp:
            file=Path(tmp)/"example.f90"
            file.write_text(source,encoding="utf-8")
            result=subprocess.run([COMPILER,*flags,"check",file],capture_output=True,text=True,encoding="utf-8",timeout=30)
            self.assertEqual(result.returncode!=0,errors,result.stdout+result.stderr)
            return result.stderr

    def test_example_and_individual_controls(self):
        source=(ROOT/"tests/programs/xunused.f90").read_text(encoding="utf-8")
        self.assertEqual(self.check(source),"")
        warnings=self.check(source,["-Wall"])
        self.assertEqual(warnings.count("warning:"),5,warnings)
        for name in ["unused_type","unused_constant","unused_variable","assigned_only","unused_procedure"]: self.assertIn("`"+name+"`",warnings)
        self.assertIn("assigned but never read",warnings)
        self.assertNotIn("`x`",warnings)
        self.assertEqual(self.check(source,["-Wall","-Wno-unused"]),"")
        warning=self.check(source,["-Wunused-type"])
        self.assertEqual(warning.count("warning:"),1,warning)
        self.assertIn("example.f90:3:9",warning)
        self.assertEqual(self.check(source,["-Wunused-type","-Wno-unused-type"]),"")
        self.assertIn("unused-type",self.check(source,["-Wno-unused-type","-Wunused-type"]))
        self.assertNotIn("unused-type",self.check(source,["-Wunused","-Wno-unused-type"]))

    def test_warning_errors(self):
        source="type :: unrelated_type\ninteger::value\nend type unrelated_type\nprint *,7\nend\n"
        self.assertEqual(self.check(source,["-Werror"]),"")
        warning=self.check(source,["-Wunused-type","-Werror"],errors=True)
        self.assertIn("error: derived type `unrelated_type`",warning)
        self.assertIn("x",self.check(source.replace("unrelated_type","x"),["-Werror=unused-type"],errors=True))
        self.assertEqual(self.check(source,["--Wall","-Wno-unused-type","--Werror"]),"")

    def test_named_constants_in_declarations(self):
        warnings=self.check("""implicit none
integer,parameter::n=3,dp=selected_real_kind(15),chars=5,unused=9
real(dp)::x
integer::a(n)
character(chars)::s
x=1.0_dp
a=0
s='abc'
print *,x,a,s
end
""",["-Wunused"])
        self.assertEqual(warnings.count("warning:"),1,warnings)
        self.assertIn("named constant `unused`",warnings)

    def test_shadowed_names_and_local_type_usage(self):
        warnings=self.check("""program main
implicit none
integer::x
type :: used_type
integer :: value
end type used_type
type(used_type)::a
call work()
a=used_type(7)
print *,a%value
contains
subroutine work()
integer::x
x=3
print *,x
end subroutine work
end program
""",["-Wunused"])
        self.assertEqual(warnings.count("warning:"),1,warnings)
        self.assertIn("variable `x` is never used",warnings)
        self.assertIn("example.f90:3:10",warnings)

    def test_private_module_members(self):
        source="""module m
implicit none
private
public :: entry,exported_constant,exported_variable
integer,parameter::exported_constant=42,dead_constant=9,used_constant=3
integer::exported_variable,dead_variable,assigned_only,used_variable
contains
subroutine entry()
used_variable=used_constant
assigned_only=7
print *,used_variable
end subroutine entry
subroutine dead_procedure()
end subroutine dead_procedure
end module m
program main
use m,only:entry
call entry()
end program
"""
        warnings=self.check(source,["-Wunused"])
        self.assertEqual(warnings.count("warning:"),4,warnings)
        for name in ["dead_constant","dead_variable","assigned_only","dead_procedure"]: self.assertIn("`"+name+"`",warnings)
        self.assertNotIn("`exported",warnings)
        self.assertNotIn("`used_",warnings)
        self.assertEqual(self.check(source,["-Wall","-Wno-unused-private"]),"")
        self.assertEqual(self.check(source,["-Wunused-private"]).count("warning:"),4)

    def test_inline_private_and_public_attributes(self):
        warnings=self.check("""module m
implicit none
integer,private::hidden
integer,parameter,private::constant=7
integer,public::exported
end module m
end
""",["-Wunused"])
        self.assertEqual(warnings.count("warning:"),2,warnings)
        self.assertIn("private module variable `hidden`",warnings)
        self.assertIn("private module named constant `constant`",warnings)
        self.assertNotIn("`exported`",warnings)

    def test_public_generic_and_bindings(self):
        warnings=self.check("""module m
implicit none
private
public::entry,box
interface entry
module procedure implementation
end interface entry
type :: box
integer::value=0
contains
procedure::touch
end type box
contains
subroutine implementation()
print *,1
end subroutine implementation
subroutine touch(self)
class(box),intent(out)::self
self%value=1
end subroutine touch
end module m
end
""",["-Wunused"])
        self.assertEqual(warnings,"")

    def test_public_operator(self):
        source=(ROOT/"tests/56_derived_operators.f90").read_text(encoding="utf-8")
        warnings=self.check(source,["-Wunused-function"])
        self.assertEqual(warnings,"")

    def test_unused_dummy_and_output(self):
        warnings=self.check("""program main
implicit none
integer::n
call work(7,n)
print *,n
contains
subroutine work(unused,output)
integer,intent(in)::unused
integer,intent(out)::output
output=42
end subroutine work
end program
""",["-Wunused-dummy-argument"])
        self.assertEqual(warnings.count("warning:"),1,warnings)
        self.assertIn("dummy argument `unused`",warnings)
        self.assertNotIn("parameter `unused`",warnings)
        self.assertNotIn("`output`",warnings)

    def test_assigned_dummies_are_useful_outputs(self):
        for intent in [",intent(out)",",intent(inout)",""]:
            with self.subTest(intent=intent):
                self.assertEqual(self.check("program main\ninteger::n\nn=0\ncall work(n)\nprint *,n\ncontains\nsubroutine work(x)\ninteger"+intent+"::x\nx=42\nend subroutine work\nend program\n",["-Wunused"]),"")

    def test_user_procedure_shadows_output_intrinsic(self):
        self.assertEqual(self.check("""program main
implicit none
real::x
x=7.0
call cpu_time(x)
contains
subroutine cpu_time(time)
real,intent(in)::time
print *,time
end subroutine cpu_time
end program
""",["-Wunused"]),"")

    def test_function_name_is_useful_result_storage(self):
        self.assertEqual(self.check("""program main
implicit none
print *,twice(7)
contains
integer function twice(i)
integer,intent(in)::i
twice=2*i
end function twice
end program
""",["-Wunused"]),"")

    def test_loop_controls_and_input(self):
        self.assertEqual(self.check("""implicit none
integer::i,input
do i=1,2
print *,7
end do
read(*,*) input
print *,input
end
""",["-Wunused"]),"")

    def test_declared_local_type_counts_as_used(self):
        warnings=self.check("""implicit none
type :: local_type
integer::value
end type local_type
type(local_type)::unused_object
end
""",["-Wunused"])
        self.assertEqual(warnings.count("warning:"),1,warnings)
        self.assertIn("variable `unused_object`",warnings)
        self.assertNotIn("derived type `local_type`",warnings)

    def test_public_signature_uses_private_type(self):
        warnings=self.check("""module m
implicit none
private
public::api
type :: hidden_type
integer::value
end type hidden_type
contains
subroutine api(x)
type(hidden_type),intent(in)::x
end subroutine api
end module m
end
""",["-Wunused-type"])
        self.assertEqual(warnings,"")

    def test_named_constant_used_only_as_literal_kind(self):
        self.assertEqual(self.check("""implicit none
integer,parameter::dp=8
real(8)::x
x=1.0_dp
print *,x
end
""",["-Wunused"]),"")

    def test_source_suppression(self):
        source=(ROOT/"tests/programs/xunused.f90").read_text(encoding="utf-8")
        self.assertNotIn("unused-type",self.check("!#allow(unused-type)\n"+source,["-Wall"]))
        self.assertIn("error: derived type",self.check("!#deny(unused-type)\n"+source,errors=True))
        self.assertIn("error: variable",self.check("!#deny(unused_program_var)\ninteger::x\nend\n",errors=True))

    def test_unused_private_recursive_cycle(self):
        warnings=self.check("""module m
implicit none
private
contains
recursive subroutine dead()
call dead()
end subroutine dead
end module m
end
""",["-Wunused-function"])
        self.assertIn("private module procedure `dead`",warnings)

    def test_unknown_warning_option(self):
        self.assertIn("unknown warning option",self.check("end\n",["-Wunused-nonsense"],errors=True))

    def test_native_multisource_locations(self):
        with tempfile.TemporaryDirectory(prefix="unused-multisource-") as tmp:
            module=Path(tmp)/"library.f90"
            main=Path(tmp)/"main.f90"
            module.write_text("module m\nprivate\ninteger::unused\nend module m\n",encoding="utf-8")
            main.write_text("use m\nprint *,7\nend\n",encoding="utf-8")
            result=subprocess.run([COMPILER,"-Wunused",module,main,"-o",Path(tmp)/"main.exe"],capture_output=True,text=True,encoding="utf-8",timeout=60)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertIn("library.f90:3:10",result.stderr)
            self.assertNotIn("f90c-sources.f90:",result.stderr)

if __name__=="__main__": unittest.main()

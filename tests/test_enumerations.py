"""F2023 nominal enumeration types in interpreter and native O0/O2."""
import subprocess
import tempfile
import unittest
from pathlib import Path
import test_fortran2023 as support
from test_record_read import COMPILER, ROOT

ENUM='''enumeration type :: colour
enumerator :: red,orange,green
end enumeration type
'''

class Enumerations(unittest.TestCase):
    check=support.Fortran2023.check
    reject=support.Fortran2023.reject

    def test_example(self):
        self.check((ROOT/"tests/programs/xenumeration.f90").read_text()," 1 2 3\n T T T\n orange or green\n 3\n 1\n 2\n 3\n")

    def test_basic_declarations_comparisons_and_parameters(self):
        self.check('implicit none\n'+ENUM+'''type(colour),parameter::chosen=orange
type(colour)::light=chosen
print *,int(light),int(colour(3)),int(huge(light))
print *,red==orange,red/=orange,red<orange,red<=red,green>red,green>=green
light=green
print *,int(light)
end
'''," 2 3 3\n F T T T T T\n 3\n")

    def test_array_assignment_and_constructor(self):
        self.check('implicit none\n'+ENUM+'''type(colour)::a(2,2),b(4)
integer::i
a=red
a(2,1)=green
b=[red,(colour(i),i=1,3)]
print *,int(a),int(b)
end
'''," 1 3 1 1 1 1 2 3\n")

    def test_generic_dispatch_preserves_nominal_identity(self):
        self.check('''module m
implicit none
'''+ENUM+'''interface code
module procedure enum_code,int_code
end interface
contains
integer function enum_code(c) result(i)
type(colour),intent(in)::c
integer::i
i=int(c)+10
end function
integer function int_code(n) result(i)
integer,intent(in)::n
integer::i
i=n+20
end function
end module
use m
implicit none
print *,code(red),code(1)
end
'''," 11 21\n")

    def test_procedure_arguments_results_and_keywords(self):
        self.check('implicit none\n'+ENUM+'''type(colour)::light=red
call set(c=light)
light=identity(c=light)
print *,int(light)
contains
subroutine set(c)
type(colour),intent(out)::c
c=green
end subroutine
function identity(c) result(r)
type(colour),intent(in)::c
type(colour)::r
r=c
end function
end
'''," 3\n")

    def test_derived_component(self):
        self.check('implicit none\n'+ENUM+'''type box
type(colour)::light
end type
type(box)::b
b%light=orange
print *,int(b%light),b%light==orange
end
'''," 2 T\n")

    def test_renamed_imports(self):
        self.check('module m\nimplicit none\n'+ENUM+'''end module
use m,only:hue=>colour,stop_colour=>red,go=>green
implicit none
type(hue)::light
light=go
print *,int(light),int(hue(1)),light>stop_colour
end
'''," 3 1 T\n")

    def test_distinct_module_types_same_spelling(self):
        self.check('module m1\n'+ENUM+'end module\nmodule m2\n'+ENUM+'''end module
use m1,only:c1=>colour,r1=>red
use m2,only:c2=>colour,r2=>red
implicit none
type(c1)::a
type(c2)::b
a=r1
b=r2
print *,int(a),int(b)
end
'''," 1 1\n")

    def test_invalid_type_operations(self):
        for statement,message in (("c=1","same enumeration type"),("n=red","same enumeration type"),("c=red+orange","arithmetic"),("print *,red<1","same enumeration type"),("print *,red","list-directed"),("c=colour(1.0)","scalar INTEGER")):
            with self.subTest(statement=statement):
                self.reject('implicit none\n'+ENUM+'type(colour)::c\ninteger::n\n'+statement+'\nend\n',message)

    def test_different_types_are_incompatible(self):
        source='implicit none\n'+ENUM+'''enumeration type :: season
enumerator :: spring,summer
end enumeration type
type(colour)::c
'''
        for statement in ("c=spring","print *,red==spring"):
            self.reject(source+statement+'\nend\n',"same enumeration type")

    def test_constant_ordinal_bounds(self):
        for index in ("0","4","-1","huge(0_8)"):
            self.reject('implicit none\n'+ENUM+f'type(colour)::c\nc=colour({index})\nend\n',"outside 1..3")

    def test_runtime_constructor_bounds(self):
        from test_record_read import build_commands
        with tempfile.TemporaryDirectory() as tmp:
            work=Path(tmp);path=work/"bounds.f90"
            path.write_text('implicit none\n'+ENUM+'integer(8)::n\ntype(colour)::c\nread *,n\nc=colour(n)\nprint *,int(c)\nend\n')
            for command in build_commands(path,work,cwd=work):
                for value in ("0\n","4\n","4294967297\n"):
                    p=subprocess.run(command,input=value,cwd=work,capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(p.returncode,0,p.stdout+p.stderr)
                    self.assertIn("ordinal outside 1..3",p.stderr)

    def test_standard_selection(self):
        with tempfile.TemporaryDirectory() as tmp:
            work=Path(tmp);path=work/"standard.f90";path.write_text(ENUM+'end\n')
            for standard,ok in (("f2018",False),("f2023",True)):
                p=subprocess.run([COMPILER,"-std="+standard,"check",path],cwd=work,capture_output=True,text=True)
                self.assertEqual(p.returncode==0,ok,p.stderr)
                if not ok:self.assertIn("enumeration types",p.stderr)

    def test_duplicates_and_end_name(self):
        self.reject('enumeration type colour\nenumerator red,red\nend enumeration type\nend\n',"names must be distinct")
        self.reject('enumeration type colour\nenumerator red\nend enumeration type other\nend\n',"name does not match")
        self.reject(ENUM+'integer::red\nend\n',"duplicate declaration")

    def test_constants_cannot_be_assigned(self):
        self.reject(ENUM+'red=orange\nend\n',"cannot assign")

    def test_polymorphic_and_storage_diagnostics(self):
        self.reject(ENUM+'class(colour)::c\nend\n',"cannot be polymorphic")
        self.reject(ENUM+'type(colour),allocatable::a(:)\nend\n',"not yet supported")

    def test_private_type_and_public_enumerator(self):
        self.check('''module m
implicit none
enumeration type,private::colour
enumerator::red,orange
end enumeration type
public::red
end module
use m,only:red
implicit none
print *,int(red)
end
'''," 1\n")

    def test_module_variable_generic_actual(self):
        self.check('module m\nimplicit none\n'+ENUM+'''type(colour)::state=orange
interface code
module procedure colour_code,integer_code
end interface
contains
integer function colour_code(x) result(n)
type(colour),intent(in)::x
integer::n
n=int(x)+10
end function
integer function integer_code(x) result(n)
integer,intent(in)::x
integer::n
n=x+20
end function
end module
use m
implicit none
print *,code(state)
state=green
print *,code(state)
end
'''," 12\n 13\n")

    def test_numeric_initialization_and_enum_subscripts_rejected(self):
        for declaration in ("integer::n=red","real::x=red"):
            self.reject('implicit none\n'+ENUM+declaration+'\nend\n',"same enumeration type")
        self.reject('implicit none\n'+ENUM+'integer::a(3)\na=1\nprint *,a(red)\nend\n',"subscripts must be INTEGER")

    def test_array_slices(self):
        self.check('implicit none\n'+ENUM+'''type(colour)::a(4),b(2)
a=red
a(2:4:2)=green
b=a(2:4:2)
print *,int(a),int(b)
end
'''," 1 3 1 3 3 3\n")

    def test_integer_kind_and_inquiry_arguments_stay_typed(self):
        for expression,message in (("int(red,kind=orange)","KIND must be"),("size(a,dim=red)","DIM and KIND"),("huge(red,red)","HUGE expects one"),("sum(a)","requires a supported intrinsic")):
            self.reject('implicit none\n'+ENUM+'type(colour)::a(2)\na=red\nprint *,'+expression+'\nend\n',message)
        for declaration in ("integer::a(red)","integer::a(1:red)","real(kind=red)::x","character(len=red)::s"):
            self.reject('implicit none\n'+ENUM+declaration+'\nend\n',"must be INTEGER")

    def test_invalid_procedure_actuals_and_cases(self):
        self.reject('implicit none\n'+ENUM+'''call f(red)
contains
subroutine f(i)
integer,intent(in)::i
end subroutine
end
''',"enumeration type mismatch")
        self.reject('implicit none\n'+ENUM+'''type(colour)::c=red
select case(c)
case(1)
end select
end
''',"same enumeration type")

    def test_constructs_are_contextual(self):
        self.check('''implicit none
integer::enumeration=3,enumerator=4
print *,enumeration,enumerator
end
'''," 3 4\n")

    def test_error_location(self):
        with tempfile.TemporaryDirectory() as tmp:
            work=Path(tmp);path=work/"bad.f90"
            path.write_text('implicit none\n'+ENUM+'type(colour)::c\nc=1\nend\n')
            p=subprocess.run([COMPILER,"check",path],cwd=work,capture_output=True,text=True)
            self.assertNotEqual(p.returncode,0)
            self.assertIn("bad.f90:6:",p.stderr)

    def test_enum_constructor_and_comparison_inside_simple(self):
        self.check('''module m
implicit none
'''+ENUM+'''contains
simple logical function later(i)
integer,intent(in)::i
later=colour(i)>red
end function
end module
use m
implicit none
print *,later(2)
end
'''," T\n")

    def test_generic_result_in_comparison_and_constructor(self):
        self.check('''module m
implicit none
'''+ENUM+'''interface choose
module procedure choose_int
end interface
interface ordinal
module procedure integer_ordinal
end interface
contains
function choose_int(i) result(c)
integer,intent(in)::i
type(colour)::c
c=colour(i)
end function
integer function integer_ordinal(i) result(n)
integer,intent(in)::i
integer::n
n=i
end function
end module
use m
implicit none
type(colour)::c
c=colour(ordinal(2))
print *,choose(1)<c,int(c)
end
'''," T 2\n")

    def test_separate_module_compilation(self):
        import os
        with tempfile.TemporaryDirectory(prefix="enum modules ") as tmp:
            work=Path(tmp)
            module=work/"colours.f90"
            module.write_text((ROOT/"tests/programs/xenumeration.f90").read_text().split("program main")[0])
            main=work/"main.f90"
            main.write_text('''use colours
implicit none
type(colour)::c
integer::i
i=2
c=advance(colour(i))
print *,int(c)
end
''')
            obj=work/("colours.obj" if os.name=="nt" else "colours.o")
            main_obj=work/("main.obj" if os.name=="nt" else "main.o")
            exe=work/("program.exe" if os.name=="nt" else "program")
            for args in (("-c",module,"-o",obj),("-c",main,"-o",main_obj),(obj,main_obj,"-o",exe)):
                p=subprocess.run([COMPILER,*map(str,args)],cwd=work,capture_output=True,text=True,timeout=90)
                self.assertEqual(p.returncode,0,p.stdout+p.stderr)
                if args[1]==module:module.unlink()
            p=subprocess.run([exe],cwd=work,capture_output=True,text=True,timeout=30)
            self.assertEqual(p.returncode,0,p.stderr)
            self.assertEqual(p.stdout," 3\n")

    def test_intrinsic_keywords(self):
        self.check('implicit none\n'+ENUM+'''type(colour)::c
c=merge(mask=.true.,fsource=green,tsource=red)
print *,int(kind=8,a=c),int(huge(x=c))
end
'''," 1 3\n")

    def test_implicitly_typed_integer_constructor_argument(self):
        self.check(ENUM+'''type(colour)::c
i=2
c=colour(i)
print *,int(c)
end
'''," 2\n")

    def test_module_subroutine_enum_argument(self):
        self.check('module m\nimplicit none\n'+ENUM+'''contains
subroutine set_colour(c)
type(colour),intent(out)::c
c=green
end subroutine
end module
use m
implicit none
type(colour)::c=red
call set_colour(c)
print *,int(c)
end
'''," 3\n")

if __name__=="__main__":unittest.main()

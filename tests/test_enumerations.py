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

    def test_typed_array_constructors(self):
        self.check('implicit none\n'+ENUM+'''type(colour)::a(4),c
integer::i
c=green
a=[colour :: red,c,[colour :: orange],colour(1)]
print *,int(a),size([colour ::])
a=[colour :: (colour(i),i=1,3),c]
print *,int(a)
end
'''," 1 3 2 1 0\n 1 2 3 3\n")

    def test_typed_constructor_generic_dispatch(self):
        self.check('module m\nimplicit none\n'+ENUM+'''interface code
module procedure enum_code,int_code
end interface
contains
integer function enum_code(c) result(n)
type(colour),intent(in)::c(:)
integer::n
n=10+size(c)
end function
integer function int_code(c) result(n)
integer,intent(in)::c(:)
integer::n
n=20+size(c)
end function
end module
use m,only:hue=>colour,r=>red,code
implicit none
print *,code([hue :: r]),code([hue ::]),code([integer ::])
end
'''," 11 10 20\n")

    def test_typed_constructor_rejects_wrong_elements(self):
        for values in ('1','red,1','spring','red,spring','a'):
            with self.subTest(values=values):
                self.reject('implicit none\n'+ENUM+'''enumeration type :: season
enumerator :: spring,summer
end enumeration type
integer::a(1)=[1]
type(colour)::c(2)
'''+f'c=[colour :: {values}]\nend\n',"typed array constructor requires elements")

    def test_typed_constructor_resolves_generic_elements(self):
        self.check('module m\nimplicit none\n'+ENUM+'''interface pick
module procedure pick_colour,pick_integer
end interface
contains
function pick_colour(c) result(r)
type(colour),intent(in)::c
type(colour)::r
r=c
end function
integer function pick_integer(i) result(r)
integer,intent(in)::i
integer::r
r=i
end function
end module
use m
implicit none
type(colour)::a(2)
a=[colour :: pick(red),pick(green)]
print *,int(a)
end
'''," 1 3\n")

    def test_generic_result_does_not_bypass_constructor_type_checks(self):
        prefix='module m\nimplicit none\n'+ENUM+'''interface identity
module procedure identity_integer
end interface
type signal
type(colour)::light
end type
contains
integer function identity_integer(i) result(r)
integer,intent(in)::i
integer::r
r=i
end function
end module
use m
implicit none
type(colour)::a(1)
type(signal)::s
'''
        self.reject(prefix+'a=[colour :: identity(1)]\nend\n',
                    "typed array constructor requires elements")
        self.reject(prefix+'s=signal(identity(1))\nend\n',
                    "constructor `signal` component")

    def test_derived_constructors_with_enumeration_components(self):
        self.check('implicit none\n'+ENUM+'''type signal
integer::id=7
type(colour)::light=orange
end type
type holder
type(signal)::value
end type
type(signal)::a,b
type(holder)::h
type(colour)::c
c=green
a=signal(4,red)
b=signal(light=c)
h=holder(signal())
print *,a%id,int(a%light),b%id,int(b%light),h%value%id,int(h%value%light)
a=signal(light=b%light,id=9)
print *,a%id,int(a%light)
end
'''," 4 1 7 3 7 2\n 9 3\n")

    def test_derived_constructor_rejects_wrong_component_types(self):
        for value in ('signal(1,1)','signal(1,spring)','signal(red,green)','signal(light=spring)','signal(light=[red])'):
            with self.subTest(value=value):
                self.reject('implicit none\n'+ENUM+'''enumeration type :: season
enumerator :: spring,summer
end enumeration type
type signal
integer::id=0
type(colour)::light=red
end type
type(signal)::s
'''+f's={value}\nend\n',"constructor `signal` component")

    def test_imported_derived_constructor(self):
        self.check('module m\nimplicit none\n'+ENUM+'''type signal
integer::id=5
type(colour)::light=red
end type
end module
use m,only:lamp=>signal,go=>green
implicit none
type(lamp)::a
a=lamp(light=go)
print *,a%id,int(a%light)
end
'''," 5 3\n")

    def test_constructor_array_components_and_distinct_enum_types(self):
        self.check('implicit none\n'+ENUM+'''enumeration type :: season
enumerator :: spring,summer
end enumeration type
type sample
type(colour)::lights(2)
type(season)::time
end type
type(sample)::s
s=sample([colour :: red,green],summer)
print *,int(s%lights),int(s%time)
end
'''," 1 3 2\n")

    def test_typed_constructor_rejects_rank_two_item(self):
        self.reject('implicit none\n'+ENUM+'''type(colour)::a(2,2),b(4)
a=red
b=[colour :: a]
end
''',"array constructor elements must be scalar or rank one")

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

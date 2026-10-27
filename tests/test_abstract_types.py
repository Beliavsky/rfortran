"""ABSTRACT contracts, DEFERRED interfaces, and concrete dispatch."""
import unittest
import subprocess
import tempfile
from pathlib import Path
import test_deallocate
from test_record_read import ROOT,COMPILER

BASE="""module m
implicit none
type,abstract::base
contains
procedure(value_iface),deferred::value
end type
"""
INTERFACE="""abstract interface
integer function value_iface(self,n) result(v)
import::base
class(base),intent(in)::self
integer,intent(in)::n
end function
end interface
"""

class AbstractTypes(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_shapes_fixture(self):
        self.check((ROOT/'tests/programs/xabstract_shapes.f90').read_text(encoding='utf-8'),
                   '6\n24\n16\n64\n24\n64\n')

    def test_missing_implementation(self):
        self.reject(BASE+"""type,extends(base)::child
end type
"""+INTERFACE+"""end module
program p
use m
end program
""", 'must implement inherited DEFERRED binding `value`')

    def test_abstract_object_and_constructor(self):
        for body,message in [('type(base)::x','cannot declare a nonpolymorphic object'),
                             ('type(base)::x(2)','cannot declare a nonpolymorphic object'),
                             ('print *,base()','cannot construct ABSTRACT type')]:
            with self.subTest(body=body):
                self.reject(BASE+INTERFACE+"""end module
program p
use m
"""+body+"""
end program
""", message)

    def test_deferred_requires_abstract_type(self):
        self.reject(BASE.replace('type,abstract::base','type::base')+INTERFACE+"""end module
program p
use m
end program
""",'requires ABSTRACT type')

    def test_override_mismatch(self):
        for decl,expected in [('real,intent(in)::n','type or kind mismatch'),
                              ('integer,intent(in)::n(:)','rank mismatch'),
                              ('integer,intent(out)::n','intent(in) attribute mismatch'),
                              ('integer,intent(in),optional::n','optional attribute mismatch')]:
            with self.subTest(decl=decl):
                source=BASE+"""type,extends(base)::child
contains
procedure::value=>child_value
end type
"""+INTERFACE+"""contains
integer function child_value(self,n) result(v)
class(child),intent(in)::self
"""+decl+"""
v=1
end function
end module
program p
use m
end program
"""
                self.reject(source,expected)

    def test_wrong_passed_object(self):
        self.reject(BASE+"""type,extends(base)::child
contains
procedure::value=>child_value
end type
"""+INTERFACE+"""contains
integer function child_value(self,n) result(v)
class(base),intent(in)::self
integer,intent(in)::n
v=n
end function
end module
program p
use m
end program
""",'requires a scalar CLASS(')

    def test_function_result_and_category(self):
        for routine,expected in [(
            """real function child_value(self,n) result(v)
class(child),intent(in)::self
integer,intent(in)::n
v=1
end function
""", 'function result: type or kind mismatch'),(
            """subroutine child_value(self,n)
class(child),intent(in)::self
integer,intent(in)::n
end subroutine
""", 'function/subroutine category'),(
            """integer function child_value(self) result(v)
class(child),intent(in)::self
v=1
end function
""", 'different number of dummy arguments'),(
            """integer function child_value(self,k) result(v)
class(child),intent(in)::self
integer,intent(in)::k
v=k
end function
""", 'must preserve dummy name')]:
            with self.subTest(expected=expected):
                self.reject(BASE+"""type,extends(base)::child
contains
procedure::value=>child_value
end type
"""+INTERFACE+'contains\n'+routine+"""end module
program p
use m
end program
""",expected)

    def test_pure_contract(self):
        self.reject(BASE+"""type,extends(base)::child
contains
procedure::value=>child_value
end type
"""+INTERFACE.replace('integer function','pure integer function')+"""contains
integer function child_value(self,n) result(v)
class(child),intent(in)::self
integer,intent(in)::n
v=n
end function
end module
program p
use m
end program
""", 'override of PURE binding')

    def test_unknown_interface_and_abstract_interface_not_implementation(self):
        self.reject(BASE+"""end module
program p
use m
end program
""", 'known explicit interface')
        self.reject(BASE.replace('procedure(value_iface),deferred::value','procedure::value=>value_iface')+INTERFACE+"""end module
program p
use m
end program
""", 'requires an implementation')

    def test_type_is_abstract_is_invalid_class_is_allowed(self):
        self.reject(BASE+INTERFACE+"""contains
subroutine f(x)
class(base),intent(in)::x
select type(x)
type is(base)
end select
end subroutine
end module
program p
use m
end program
""", 'TYPE IS guard cannot name an ABSTRACT type')
        self.check("""module m
implicit none
type,abstract::base
integer::v
end type
type,extends(base)::child
end type
contains
subroutine f(x)
class(base),intent(in)::x
select type(y=>x)
class is(base)
print *,y%v
end select
end subroutine
end module
program p
use m
implicit none
type(child)::a
a%v=7
call f(a)
end program
""", '7\n')

    def test_non_overridable_binding(self):
        self.reject("""module m
implicit none
type,abstract::base
contains
procedure,non_overridable::value=>base_value
end type
type,extends(base)::child
contains
procedure::value=>child_value
end type
contains
integer function base_value(self) result(n)
class(base),intent(in)::self
n=1
end function
integer function child_value(self) result(n)
class(child),intent(in)::self
n=2
end function
end module
program p
use m
end program
""", 'cannot override NON_OVERRIDABLE binding')

    def test_standard_gate(self):
        source=(ROOT/'tests/programs/xabstract_shapes.f90').read_text(encoding='utf-8')
        with tempfile.TemporaryDirectory(prefix='abstract-standard-') as tmp:
            path=Path(tmp)/'example.f90'
            path.write_text(source,encoding='utf-8')
            for std,accepted in [('f95',False),('f2003',True),('f2023',True)]:
                result=subprocess.run([COMPILER,'-std='+std,'parse',path,Path(tmp)/'ast.json'],capture_output=True,text=True,timeout=30)
                self.assertEqual(result.returncode==0,accepted,result.stderr)
                if not accepted:
                    self.assertIn('ABSTRACT derived types requires Fortran 2003',result.stderr)
                    self.assertIn('DEFERRED bindings requires Fortran 2003',result.stderr)

    def test_separate_module(self):
        source=(ROOT/'tests/programs/xabstract_shapes.f90').read_text(encoding='utf-8')
        module,main=source.split('program test_abstract_shapes',1)
        main='program test_abstract_shapes'+main
        with tempfile.TemporaryDirectory(prefix='abstract-module-') as tmp:
            work=Path(tmp)
            (work/'m.f90').write_text(module,encoding='utf-8')
            (work/'main.f90').write_text(main,encoding='utf-8')
            for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                            [COMPILER,'main.f90','m.obj','-o','main.exe']):
                result=subprocess.run(command,cwd=work,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
            result=subprocess.run([work/'main.exe'],capture_output=True,text=True,timeout=15)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.split(),['6.0','24.0','16.0','64.0','24.0','64.0'])

    def test_use_association_private_interface_and_kind_alias(self):
        self.check("""module contracts
implicit none
private
public::base
integer,parameter::dp=kind(1.0d0)
type,abstract::base
contains
procedure(evaluate_iface),deferred::evaluate
end type
abstract interface
function evaluate_iface(self,x) result(v)
import::base,dp
class(base),intent(in)::self
real(kind=dp),intent(in)::x(:)
real(kind=dp)::v
end function
end interface
end module
module implementations
use contracts,only:parent=>base
implicit none
type,extends(parent)::box
real(kind=kind(1.0d0))::offset=10.0d0
contains
procedure::evaluate=>evaluate_box
end type
contains
double precision function evaluate_box(self,x) result(v)
class(box),intent(in)::self
double precision,intent(in)::x(:)
v=sum(x)+self%offset
end function
subroutine show(x)
class(parent),intent(in)::x
print *,x%evaluate([1.0d0,2.0d0])
end subroutine
end module
program p
use implementations,only:box,show
implicit none
type(box)::x
call show(x)
end program
""", '13\n')

    def test_binding_diagnostic_line(self):
        source=BASE+"""end module
program p
use m
end program
"""
        with tempfile.TemporaryDirectory(prefix='abstract-diagnostic-') as tmp:
            path=Path(tmp)/'example.f90'
            path.write_text(source,encoding='utf-8')
            result=subprocess.run([COMPILER,'run',path],capture_output=True,text=True,timeout=30)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('known explicit interface',result.stderr)
            self.assertIn('example.f90:5:',result.stderr)

    def test_deferred_attribute_errors(self):
        for declaration,expected in [
            ('procedure,deferred::value','require an explicit procedure interface'),
            ('procedure(value_iface),deferred,non_overridable::value','cannot be combined'),
            ('procedure(value_iface),deferred,nopass::value','NOPASS deferred bindings')]:
            with self.subTest(declaration=declaration):
                self.reject(BASE.replace('procedure(value_iface),deferred::value',declaration)+INTERFACE+"""end module
program p
use m
end program
""",expected)

    def test_abstract_library_with_no_concrete_instance(self):
        self.check(BASE+INTERFACE+"""end module
program p
use m
print *,7
end program
""",'7\n')

    def test_local_kind_constants_do_not_leak_between_interfaces(self):
        source=BASE+"""type,extends(base)::child
contains
procedure::value=>child_value
end type
abstract interface
function value_iface(self,n) result(v)
import::base
integer,parameter::precision=kind(1.0d0)
class(base),intent(in)::self
integer,intent(in)::n
real(kind=precision)::v
end function
end interface
contains
function child_value(self,n) result(v)
class(child),intent(in)::self
integer,intent(in)::n
integer,parameter::precision=kind(1.0)
real(kind=precision)::v
v=real(n)
end function
end module
program p
use m
end program
"""
        self.reject(source,'function result: type or kind mismatch')

    def test_abstract_components_and_typeof(self):
        self.reject("""module m
implicit none
type,abstract::base
integer::v
end type
type box
type(base)::component
end type
end module
program p
use m
end program
""",'cannot declare a nonpolymorphic object')
        self.reject("""module m
implicit none
type,abstract::base
integer::v
end type
contains
subroutine f(x)
class(base),intent(in)::x
typeof(x)::y
end subroutine
end module
program p
use m
end program
""", 'TYPEOF cannot declare a nonpolymorphic object')

    def test_default_character_length_matches_explicit_one(self):
        self.check("""module m
implicit none
type,abstract::base
contains
procedure(fill_iface),deferred::fill
end type
type,extends(base)::child
integer::v=1
contains
procedure::fill=>fill_child
end type
abstract interface
subroutine fill_iface(self,ch)
import::base
class(base),intent(in)::self
character,intent(out)::ch
end subroutine
end interface
contains
subroutine fill_child(self,ch)
class(child),intent(in)::self
character(len=1),intent(out)::ch
ch='x'
end subroutine
subroutine show(x)
class(base),intent(in)::x
character::ch
call x%fill(ch)
print '(a)',ch
end subroutine
end module
program p
use m
implicit none
type(child)::a
call show(a)
end program
""", 'x\n')

    def test_simple_contract(self):
        self.reject(BASE+"""type,extends(base)::child
contains
procedure::value=>child_value
end type
"""+INTERFACE.replace('integer function','simple integer function')+"""contains
pure integer function child_value(self,n) result(v)
class(child),intent(in)::self
integer,intent(in)::n
v=n
end function
end module
program p
use m
end program
""", 'override of SIMPLE binding')

if __name__=='__main__':
    unittest.main()

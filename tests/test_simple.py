"""Fortran 2023 SIMPLE procedure restrictions and execution."""
import subprocess
import tempfile
import unittest
from pathlib import Path
import test_fortran2023 as support
from test_record_read import COMPILER, ROOT

class SimpleTests(unittest.TestCase):
    check = support.Fortran2023.check
    reject = support.Fortran2023.reject

    def test_example(self):
        self.check((ROOT / "tests/programs/xsimple_procedures.f90").read_text(), "  2.000  4.000  6.000\n 4\n")

    def test_prefix_orders_and_functions(self):
        for prefix in ("simple integer", "integer simple", "simple recursive integer", "integer recursive simple"):
            with self.subTest(prefix=prefix):
                self.check(f"""implicit none
print *, twice(3)
contains
{prefix} function twice(i) result(j)
integer,intent(in)::i
j=2*i
end function
end
""", " 6\n")

    def test_subroutine_and_intrinsic(self):
        self.check("""implicit none
integer::p
call find(p)
print *,p
contains
simple subroutine find(p)
integer,intent(out)::p
p=0
call split('abc,def',',',p)
end subroutine
end
""", " 4\n")

    def test_module_constants_and_use_rename(self):
        self.check("""module constants
integer,parameter::factor=3
integer::unused=99
contains
simple integer function times(i) result(j)
integer,intent(in)::i
j=factor*i
end function
end module
module consumer
use constants,only:scale=>times,factor
contains
simple integer function f(i) result(j)
integer,intent(in)::i
j=scale(i)+factor
end function
end module
program main
use consumer,only:f
implicit none
print *,f(4)
end
""", " 15\n")

    def test_host_constant_and_local_shadow(self):
        self.check("""implicit none
integer,parameter::n=4
integer::outside=9
print *,f(2)
contains
simple integer function f(i) result(j)
integer,intent(in)::i
integer::outside
outside=i+1
j=n+outside
end function
end
""", " 7\n")

    def test_elemental(self):
        self.check("""implicit none
print *,twice([1,2,3])
contains
real simple elemental function twice(i) result(j)
integer,intent(in)::i
j=2.0*i
end function
end
""", " 2.0 4.0 6.0\n")

    def test_nonlocal_reads_with_location(self):
        for declaration, statement in (("integer::outside=4", "j=outside+i"),
                                       ("integer::outside(2)=[4,5]", "j=outside(i)"),
                                       ("character(4)::outside='abcd'", "j=len_trim(outside)")):
            source=f"""implicit none
{declaration}
print *,f(1)
contains
simple integer function f(i) result(j)
integer,intent(in)::i
{statement}
end function
end
"""
            self.reject(source,"SIMPLE")
        self.reject("""module m
integer::outside=4
contains
simple integer function f() result(j)
j=outside
end function
end module
end
""", "nonlocal variable")

    def test_pure_is_not_simple(self):
        self.reject("""implicit none
print *,f(2)
contains
simple integer function f(i) result(j)
integer,intent(in)::i
j=ordinary(i)
end function
pure integer function ordinary(i) result(j)
integer,intent(in)::i
j=i
end function
end
""", "non-SIMPLE")

    def test_internal_procedures_must_be_simple(self):
        self.reject("""simple integer function f() result(j)
j=1
contains
pure integer function helper() result(k)
k=2
end function
end function
end
""", "internal procedures")

    def test_pure_restrictions(self):
        for statement, error in (("print *,i", "external I/O"),
                                 ("i=3", "dummy argument"),
                                 ("call random_number(x)", "non-SIMPLE")):
            self.reject(f"""implicit none
print *,f(1)
contains
simple integer function f(i) result(j)
integer,intent(in)::i
real::x
{statement}
j=i
end function
end
""", error)

    def test_simple_dummy_procedure(self):
        self.check("""implicit none
print *,apply(twice,3)
contains
simple integer function twice(i) result(j)
integer,intent(in)::i
j=2*i
end function
simple integer function apply(f,i) result(j)
abstract interface
simple integer function callback(n)
integer,intent(in)::n
end function
end interface
procedure(callback)::f
integer,intent(in)::i
j=f(i)
end function
end
""", " 6\n")

    def test_dummy_requires_simple_interface(self):
        self.reject("""simple integer function apply(f,i) result(j)
integer,external::f
integer,intent(in)::i
j=f(i)
end function
end
""", "explicit SIMPLE interface")

    def test_inferred_dummy_requires_simple_interface(self):
        self.reject("""simple integer function apply(f,i) result(j)
integer,intent(in)::i
j=f(i)
end function
end
""", "explicit SIMPLE interface")

    def test_ordinary_actual_for_simple_dummy(self):
        self.reject("""implicit none
print *,apply(twice,3)
contains
pure integer function twice(i) result(j)
integer,intent(in)::i
j=2*i
end function
simple integer function apply(f,i) result(j)
interface
simple integer function f(n)
integer,intent(in)::n
end function
end interface
integer,intent(in)::i
j=f(i)
end function
end
""", "non-SIMPLE")

    def test_tokenize_and_move_alloc(self):
        self.check("""implicit none
integer,allocatable::first(:),last(:),moved(:)
call parts('a,b',first,last)
call move(first,moved)
print *,sum(moved)+sum(last)
contains
simple subroutine parts(text,first,last)
character(*),intent(in)::text
integer,allocatable,intent(out)::first(:),last(:)
call tokenize(text,',',first,last)
end subroutine
simple subroutine move(first,moved)
integer,allocatable,intent(inout)::first(:)
integer,allocatable,intent(out)::moved(:)
call move_alloc(first,moved)
end subroutine
end
""", " 8\n")

    def test_generic_resolution(self):
        source="""module m
interface choose
module procedure choose_i,choose_r
end interface
contains
simple integer function choose_i(i) result(j)
integer,intent(in)::i
j=i+1
end function
pure real function choose_r(i) result(j)
real,intent(in)::i
j=i+1.0
end function
simple integer function wrapper(i) result(j)
integer,intent(in)::i
j=choose(i)
end function
end module
program main
use m,only:wrapper
print *,wrapper(4)
end
"""
        self.check(source," 5\n")
        self.reject(source.replace("j=choose(i)","j=int(choose(real(i)))"), "non-SIMPLE")

    def test_type_bound_calls(self):
        source="""module m
implicit none
type t
integer::n
contains
procedure::value
end type
contains
simple integer function value(self) result(j)
class(t),intent(in)::self
j=self%n
end function
simple integer function wrapper(x) result(j)
type(t),intent(in)::x
j=x%value()
end function
end module
program main
use m
print *,wrapper(t(3))
end
"""
        self.check(source," 3\n")
        self.reject(source.replace("simple integer function value", "pure integer function value"), "non-SIMPLE")

    def test_derived_result_prefix_orders(self):
        source="""module m
implicit none
type t
integer::n
end type
contains
TYPE_PREFIX function make(i) result(x)
integer,intent(in)::i
x%n=i
end function
end module
program main
use m
implicit none
type(t)::a
a=make(3)
print *,a%n
end
"""
        for prefix in ("simple type(t)","type(t) simple"):
            self.check(source.replace("TYPE_PREFIX",prefix)," 3\n")

    def test_override_characteristic(self):
        source="""module m
implicit none
type base
contains
procedure::f
end type
type,extends(base)::child
contains
procedure::f=>g
end type
contains
simple integer function f(self) result(j)
class(base),intent(in)::self
j=1
end function
pure integer function g(self) result(j)
class(child),intent(in)::self
j=2
end function
end module
end
"""
        self.reject(source,"same SIMPLE characteristic")
        self.reject(source.replace("simple integer function f","pure integer function f").replace("pure integer function g","simple integer function g"),"same SIMPLE characteristic")

    def test_error_location(self):
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/"nonlocal.f90"
            path.write_text("""integer::outside=1
contains
simple integer function f() result(j)
j=outside
end function
end
""")
            result=subprocess.run([COMPILER,"check",path],capture_output=True,text=True)
            self.assertNotEqual(result.returncode,0)
            self.assertIn("nonlocal variable",result.stderr)
            self.assertIn("nonlocal.f90:4:1",result.stderr)

    def test_separate_module_compilation(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory=Path(tmp)
            module=directory/"m.f90"
            module.write_text("""module m
integer,parameter::factor=3
contains
simple integer function times(i) result(j)
integer,intent(in)::i
j=factor*i
end function
end module
""")
            def command(*args):
                return subprocess.run([COMPILER,*map(str,args)],cwd=directory,capture_output=True,text=True,timeout=60)
            compiled=command("-c",module)
            self.assertEqual(compiled.returncode,0,compiled.stderr)
            module.unlink()
            main=directory/"main.f90"
            main.write_text("""use m,only:times
implicit none
print *,wrapped(4)
contains
simple integer function wrapped(i) result(j)
integer,intent(in)::i
j=times(i)
end function
end
""")
            checked=command("check",main)
            self.assertEqual(checked.returncode,0,checked.stderr)
            rejected=command("-std=f2018","check",main)
            self.assertNotEqual(rejected.returncode,0)
            self.assertIn("SIMPLE",rejected.stderr)
            obj=next(directory.glob("m.obj"),directory/"m.o")
            exe=directory/"program.exe"
            built=command(main,obj,"-o",exe)
            self.assertEqual(built.returncode,0,built.stderr)
            ran=subprocess.run([exe],cwd=directory,capture_output=True,text=True)
            self.assertEqual(ran.returncode,0,ran.stderr)
            self.assertEqual(ran.stdout," 12\n")

    def test_invalid_prefix_combinations(self):
        for prefix in ("simple pure", "pure simple", "simple impure", "simple simple", "simple integer pure"):
            self.reject(f"{prefix} function f() result(j)\ninteger::j\nj=1\nend function\nend\n", "SIMPLE")

    def test_contextual_identifier(self):
        self.check("""implicit none
integer::simple
simple=3
print *,simple
end
""", " 3\n")

    def test_standard_modes(self):
        source="simple integer function f() result(j)\nj=1\nend function\nend\n"
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/"standard.f90"
            path.write_text(source)
            for standard in ("f90","f95","f2003","f2008","f2018","f2023"):
                result=subprocess.run([COMPILER,"check",path,f"-std={standard}"],capture_output=True,text=True)
                if standard=="f2023": self.assertEqual(result.returncode,0,result.stderr)
                else:
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn("SIMPLE",result.stderr)

if __name__=="__main__": unittest.main()

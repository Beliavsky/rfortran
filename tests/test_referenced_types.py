"""Fortran 2023 TYPEOF/CLASSOF declarations across interpreter and native backends."""
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
import test_fortran2023 as shared
from test_record_read import COMPILER, ROOT

class ReferencedTypes(unittest.TestCase):
    check=shared.Fortran2023.check
    reject=shared.Fortran2023.reject

    def test_dimension_attribute_and_entity_forms(self):
        for declarations in (
            'typeof(mold),dimension(2)::a,b(3)\ntypeof(i),dimension(2,2)::x',
            'typeof(mold)::a(2),b(3)\ntypeof(i)::x(2,2)',
        ):
            with self.subTest(declarations=declarations):
                self.check('''implicit none
type item
integer::value
end type
type(item)::mold
integer::i
'''+declarations+'''
a%value=2
b%value=3
x=4
print *,size(a),size(b),sum(a%value),sum(b%value),sum(x)
end
''',' 2 3 4 9 16\n')

    def test_examples(self):
        for name,expected in [('xtypeof.f90'," 2 T 5\nready\n"),('xclassof.f90'," 7\n")]:
            with self.subTest(name=name):self.check((ROOT/'tests/programs'/name).read_text(),expected)

    def test_intrinsics_parameters_and_independent_shape(self):
        self.check('''use iso_fortran_env,only:real64,int64
implicit none
real(real64)::a(3)
integer(int64)::i
logical::mask
complex(real64)::z
character(4)::text
typeof(a)::b,c(2)
typeof(i),parameter::j=12_int64
typeof(mask)::flag
typeof(z)::zz
typeof(text)::s
a=1.;b=2.;c=[3.,4.];i=5;flag=.true.;zz=(1.,2.);s='ok'
print *,kind(b)==real64,kind(j)==int64,kind(zz)==real64,flag
print *,size(c),len(s),j
print '(a)',s
end
'''," T T T T\n 2 4 12\nok  \n")

    def test_derived_components_and_array_elements(self):
        self.check('''implicit none
type::point
integer::x
real::y
end type
type::wrapper
type(point)::p
end type
type(point)::a(2)
type(wrapper)::w
typeof(a)::b
typeof(a(1))::c(2)
typeof(w%p)::d
typeof(w%p%x)::k
b%x=7;b%y=2.
c(1)=b;c(2)=b
d=b;k=9
print *,d%x,k,c(2)%x
end
'''," 7 9 7\n")

    def test_module_import_rename_reexport_and_host(self):
        self.check('''module m
implicit none
integer,parameter,private::rk=kind(1.d0)
real(rk)::a
character(3)::text
typeof(a)::b
contains
subroutine work()
typeof(text)::s
s='yes'
print '(a)',s
end subroutine
end module
module n
use m,only:renamed=>a
typeof(renamed)::z
end module
program main
use m,only:a,work
use n,only:z
implicit none
typeof(a)::x
x=1.;z=2.
print *,kind(x)==kind(1.d0),kind(z)==kind(x)
call work()
call inner()
contains
subroutine inner()
typeof(x)::y
y=3.
print *,kind(y)==kind(x)
end subroutine
end
'''," T T\nyes\n T\n")

    def test_public_object_with_private_derived_type(self):
        self.check('''module m
implicit none
private
public::a
type::t
integer::value
end type
type(t)::a
end module
program main
use m,only:a
implicit none
typeof(a)::b
b%value=7
print *,b%value
end
'''," 7\n")
        self.reject('''module m
implicit none
private
public::a
type::t
integer::value
end type
type(t)::a
end module
program main
use m,only:a
implicit none
type(t)::b
end
''','not accessible')

    def test_assumed_character_length(self):
        self.check('''implicit none
call work('abc')
call work('longer')
contains
subroutine work(text)
character(*),intent(in)::text
typeof(text)::copy
copy=text
print *,len(copy)
print '(a)',copy
end subroutine
end
'''," 3\nabc\n 6\nlonger\n")

    def test_deferred_character_length_and_optional_fixed_length(self):
        self.check('''implicit none
character(:),allocatable::a
typeof(a),allocatable::b
a='abc';b='longer'
print *,len(a),len(b)
print '(a)',b
call work()
contains
subroutine work(text)
character(3),optional::text
typeof(text)::copy
copy='yes'
print '(a)',copy
end subroutine
end
'''," 3 6\nlonger\nyes\n")

    def test_allocation_attributes_are_independent(self):
        self.check('''implicit none
integer,allocatable::a(:)
typeof(a)::scalar,values(2)
typeof(a),allocatable::b(:)
scalar=3;values=[4,5]
allocate(b(2));b=values
print *,scalar,b(1),b(2)
end
'''," 3 4 5\n")

    def test_pointer_attributes_are_independent(self):
        self.check('''implicit none
integer,target::value
integer,pointer::p
typeof(value),pointer::q
typeof(p)::ordinary
value=7;q=>value;ordinary=6
print '(i0,1x,i0)',ordinary,q
end
''',"6 7\n")

    def test_typeof_component_declarations_and_block_shadowing(self):
        self.check('''implicit none
integer::model
type::sample
typeof(model)::value
end type
type(sample)::s
typeof(model)::i
s%value=4;i=2
block
character(3)::model
typeof(model)::text
text='yes'
print '(a)',text
end block
print *,i,s%value
end
''',"yes\n 2 4\n")

    def test_classof_dummy_and_typeof_polymorphic_source(self):
        self.check('''module m
implicit none
type::base
integer::value
end type
type,extends(base)::child
integer::extra
end type
contains
subroutine work(a,b)
class(base),intent(in)::a
classof(a),intent(in)::b
typeof(a)::copy
copy%value=a%value+b%value
print *,copy%value
end subroutine
end module
program main
use m
implicit none
type(child)::a,b
a%value=3;a%extra=10;b%value=4;b%extra=20
call work(a,b)
end
'''," 7\n")

    def test_chained_declarations_and_contextual_names(self):
        self.check('''implicit none
integer::typeof,classof
typeof(typeof)::x
typeof(x)::y
typeof=2;classof=3;x=4;y=5
print *,typeof,classof,x,y
end
'''," 2 3 4 5\n")
        self.check('''implicit none
print *,typeof(3),classof(4)
contains
integer function typeof(i)
integer::i
typeof=i+1
end function
integer function classof(i)
integer::i
classof=i+2
end function
end
'''," 4 6\n")

    def test_function_result_declarations(self):
        self.check('''implicit none
print *,twice(3)
contains
function twice(a) result(b)
integer,intent(in)::a
typeof(a)::b
b=2*a
end function
end
'''," 6\n")

    def test_implicit_typing(self):
        self.check('''typeof(index)::i
typeof(value)::x
i=3;x=4.
print *,i,kind(x)==kind(1.)
end
'''," 3 T\n")

    def test_invalid_references_and_classof_constraints(self):
        cases=[
            ("type::t\ninteger::i\nend type\ntypeof(t)::x\nend\n","not a type name"),
            ("typeof(f)::x\ncontains\nreal function f()\nf=1.\nend function\nend\n","not a procedure"),
            ("subroutine s(a)\nclass(*),intent(in)::a\nclassof(a),allocatable::b\nend\n","requires unlimited polymorphism"),
            ("subroutine s(a)\nclass(*),intent(in)::a\ntypeof(a)::b\nend\n","must not be unlimited polymorphic"),
            ("implicit none\ntypeof(missing)::x\nend\n","no previously declared type"),
            ("implicit none\ninteger::i\ntypeof(i+1)::x\nend\n","requires a data reference"),
            ("implicit none\ninteger::i\nclassof(i),allocatable::x\nend\n","must not have intrinsic type"),
            ("implicit none\ntype::t\ninteger::i\nend type\ntype(t)::a\nclassof(a)::b\nend\n","must be a dummy argument"),
            ("subroutine s(a)\ncharacter(*),optional::a\ntypeof(a)::b\nend\n","OPTIONAL reference"),
        ]
        for source,message in cases:
            with self.subTest(source=source): self.reject(source,message)

    def test_separately_compiled_module(self):
        with tempfile.TemporaryDirectory() as tmp:
            work=Path(tmp);module=work/'model.f90';main=work/'main.f90'
            module.write_text("module model_mod\nimplicit none\ninteger,private,parameter::rk=kind(1.d0)\nreal(rk)::model\ntypeof(model)::other\nend module\n")
            result=subprocess.run([COMPILER,'-c',module],cwd=work,capture_output=True,text=True,timeout=60)
            self.assertEqual(result.returncode,0,result.stderr)
            main.write_text("use model_mod,only:model\nimplicit none\ntypeof(model)::x\nx=1.\nprint *,kind(x)==kind(1.d0)\nend\n")
            commands=[[COMPILER,'run',main]]
            module_object=work/('model.obj' if os.name=='nt' else 'model.o')
            for opt in (0,2):
                obj=work/f'main{opt}.obj';exe=work/f'main{opt}.exe'
                for command in ([COMPILER,'-O',str(opt),'emit-obj',main,obj],
                                [COMPILER,'link',obj,module_object,'-o',exe]):
                    result=subprocess.run(command,cwd=work,capture_output=True,text=True,timeout=60)
                    self.assertEqual(result.returncode,0,result.stderr)
                commands.append([exe])
            for command in commands:
                result=subprocess.run(command,cwd=work,capture_output=True,text=True,timeout=30)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(result.stdout," T\n")

    def test_standard_modes_and_diagnostic_locations(self):
        with tempfile.TemporaryDirectory() as tmp:
            work=Path(tmp);source=work/'example.f90'
            source.write_text("implicit none\ninteger::i\ntypeof(i)::j\nj=1\nprint *,j\nend\n")
            for standard in ('f90','f95','f2003','f2008','f2018'):
                result=subprocess.run([COMPILER,'-std='+standard,'check',source],cwd=work,capture_output=True,text=True,timeout=30)
                self.assertNotEqual(result.returncode,0)
                self.assertIn('TYPEOF/CLASSOF',result.stderr)
                self.assertIn('example.f90:3:',result.stderr)
            result=subprocess.run([COMPILER,'-std=f2023','check',source],cwd=work,capture_output=True,text=True,timeout=30)
            self.assertEqual(result.returncode,0,result.stderr)
            source.write_text("implicit none\ninteger::i\nclassof(i),allocatable::j\nend\n")
            result=subprocess.run([COMPILER,'check',source],cwd=work,capture_output=True,text=True,timeout=30)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('example.f90:3:',result.stderr)

if __name__=='__main__':unittest.main()

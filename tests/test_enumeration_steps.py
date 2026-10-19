"""NEXT/PREVIOUS enumeration intrinsics in interpreter and native O0/O2."""
from pathlib import Path
import subprocess
import tempfile
import unittest

import test_enumerations as enums
import test_fortran2023 as support
from test_record_read import COMPILER, ROOT, build_commands


class EnumerationSteps(unittest.TestCase):
    check = support.Fortran2023.check
    reject = support.Fortran2023.reject

    def test_example(self):
        self.check((ROOT/'tests/programs/xenumeration_steps.f90').read_text(),
                   ' 2\n 2\n 3 T\n 1 T\n 2 3 3\n 0 0 1\n')

    def test_scalar_steps_status_and_keywords(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour)::c
integer::status
integer(8)::wide
c=next(a=red)
print *,int(c),int(previous(a=green))
c=next(stat=status,a=orange)
print *,int(c),status
c=next(green,status)
print *,int(c),status>0
c=previous(red,stat=wide)
print *,int(c),wide>0
c=previous(orange,wide)
print *,int(c),wide
end
''', ' 2 2\n 3 0\n 3 T\n 1 T\n 1 0\n')

    def test_array_steps_and_status(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour)::a(3),b(3)
integer::status(3)
a=[red,orange,green]
b=next(a,stat=status)
print *,int(b),status
b=previous(a,status)
print *,int(b),status
b=next(red,status)
print *,int(b),status
b=next([red,orange,red])
print *,int(b)
end
''', ' 2 3 3 0 0 1\n 1 1 2 1 0 0\n 2 2 2 0 0 0\n 2 3 2\n')

    def test_pure_and_simple_functions(self):
        self.check('implicit none\n'+enums.ENUM+'''print *,int(f(red)),int(g(orange))
contains
pure function f(a) result(b)
type(colour),intent(in)::a
type(colour)::b
integer::status
b=next(a,status)
end function
simple function g(a) result(b)
type(colour),intent(in)::a
type(colour)::b
b=previous(a)
end function
end
''', ' 2 1\n')

    def test_boundary_errors_have_call_location(self):
        for operation,actual in [('next','green'),('previous','red')]:
            with self.subTest(operation=operation), tempfile.TemporaryDirectory() as tmp:
                work=Path(tmp); path=work/'boundary.f90'
                source='implicit none\n'+enums.ENUM+'type(colour)::c\n'+f'c={actual}\nc={operation}(c)\nprint *,int(c)\nend\n'
                path.write_text(source)
                line=next(i for i,s in enumerate(source.splitlines(),1) if s.startswith(f'c={operation}'))
                for command in build_commands(path,work,cwd=work):
                    p=subprocess.run(command,cwd=work,capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(p.returncode,0,p.stdout+p.stderr)
                    self.assertIn(operation.upper()+':',p.stderr)
                    self.assertIn(f'boundary.f90:{line}',p.stderr)

    def test_invalid_arguments(self):
        for call,message in [('next(1)','enumeration type'),('previous()','expects'),
                             ('next(red,1)','writable'),('next(red,x)','INTEGER'),
                             ('next(red,small)','decimal exponent range'),('next(red,k)','named constant'),
                             ('next(a,status)','conformable array')]:
            with self.subTest(call=call):
                self.reject('implicit none\n'+enums.ENUM+'''type(colour)::c,a(2)
integer::status
integer,parameter::k=0
integer(1)::small
real::x
a=red
'''+f'c={call}\nend\n',message)

    def test_multidimensional_arrays_sections_and_integer_kinds(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour)::a(2,2),b(2,2)
integer(2)::status(2,2)
integer(8)::wide(4)
a=orange
a(2,2)=green
b=next(a,status)
print *,int(b),status
wide=-1
b(:,1)=previous(a(:,2),wide(2:3))
print *,int(b(:,1)),wide
end
''', ' 3 3 3 3 0 0 0 1\n 1 2 -1 0 0 -1\n')

    def test_scalar_status_element_and_component(self):
        self.check('implicit none\n'+enums.ENUM+'''type info
integer(8)::status
end type
type(info)::obj
type(colour)::c
integer::status(2)
c=next(green,status(2))
c=previous(red,obj%status)
print *,int(c),status(2)>0,obj%status>0
end
''', ' 1 T T\n')

    def test_user_procedure_names_take_priority(self):
        self.check('implicit none\n'+enums.ENUM+'''print *,next(1),previous(2)
contains
integer function next(i) result(r)
integer,intent(in)::i
integer::r
r=i+10
end function
integer function previous(i) result(r)
integer,intent(in)::i
integer::r
r=i+20
end function
end
''', ' 11 22\n')

    def test_constant_initializer_and_nested_steps(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour),parameter::middle=next(red)
type(colour)::c
c=previous(next(middle))
print *,int(middle),int(c)
end
''', ' 2 2\n')

    def test_optional_status_forwarding(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour)::c
integer::status
call step(red,c)
print *,int(c)
call step(green,c,status)
print *,int(c),status>0
contains
subroutine step(a,b,status)
type(colour),intent(in)::a
integer,intent(out),optional::status
type(colour),intent(out)::b
b=next(a,status)
end subroutine
end
''', ' 2\n 3 T\n')

    def test_pure_cannot_modify_nonlocal_status(self):
        self.reject('implicit none\n'+enums.ENUM+'''integer::status
print *,int(f(red))
contains
pure function f(a) result(b)
type(colour),intent(in)::a
type(colour)::b
b=next(a,status)
end function
end
''', 'nonlocal variable')

    def test_status_cannot_be_intent_in_or_vector_subscripted(self):
        self.reject('implicit none\n'+enums.ENUM+'''type(colour)::c
integer::s
call f(c,s)
contains
subroutine f(c,s)
type(colour),intent(out)::c
integer,intent(in)::s
c=next(red,s)
end subroutine
end
''', 'INTENT(IN)')
        self.reject('implicit none\n'+enums.ENUM+'''type(colour)::c(2)
integer::s(2)
c=next(red,s([1,2]))
end
''', 'vector subscript')

    def test_status_call_inside_conditional_is_lazy(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour)::c
integer::status,choice
read *,choice
status=42
c=(choice==1 ? next(green,status) : red)
print *,int(c),status
end
''', ' 1 42\n',data='0\n')

    def test_generic_named_next(self):
        self.check('module m\nimplicit none\n'+enums.ENUM+'''interface next
module procedure plus_ten
end interface
contains
integer function plus_ten(i) result(r)
integer,intent(in)::i
integer::r
r=i+10
end function
end module
use m
implicit none
print *,next(1)
end
''', ' 11\n')

    def test_module_status_and_named_kind(self):
        self.check('module m\nuse iso_fortran_env,only:int64\nimplicit none\n'+enums.ENUM+'''integer(int64)::status
end module
use m
implicit none
type(colour)::c
c=next(green,status)
print *,int(c),status>0
end
''', ' 3 T\n')

    def test_nonconforming_status_shape(self):
        self.reject('implicit none\n'+enums.ENUM+'''type(colour)::a(3),b(3)
integer::status(2)
a=red
b=next(a,status)
end
''', 'nonconforming')

    def test_source_group_boundary_location(self):
        with tempfile.TemporaryDirectory() as tmp:
            work=Path(tmp); module=work/'colours.f90'; main=work/'main.f90'
            module.write_text('module m\nimplicit none\n'+enums.ENUM+'end module\n')
            main.write_text('use m\nimplicit none\ntype(colour)::c\nc=green\nc=next(c)\nprint *,int(c)\nend\n')
            p=subprocess.run([str(COMPILER),str(module),str(main),'-o',str(work/'boundary.exe'),'--run'],
                             cwd=work,capture_output=True,text=True,timeout=30)
            self.assertNotEqual(p.returncode,0,p.stdout+p.stderr)
            self.assertIn('NEXT:',p.stderr)
            self.assertIn('main.f90:5',p.stderr)

    def test_zero_sized_call(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour)::a(0),b(0)
integer::status(0)
b=next(a,status)
print *,size(b),size(status)
end
''', ' 0 0\n')


if __name__=='__main__':
    unittest.main()

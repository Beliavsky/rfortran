"""Borrowed runtime polymorphic allocatable descriptors across procedure calls."""
import unittest
import tempfile
import subprocess
from pathlib import Path
import test_deallocate
from test_record_read import ROOT,COMPILER

FIXTURE=(ROOT/'tests/programs/xpolymorphic_allocatable_dummies.f90').read_text()
PREFIX=FIXTURE.split('program p')[0]

class PolymorphicAllocatableDummies(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_factory_fixture(self):
        self.check(FIXTURE,'F\nF\nT\n12\nF\nT\n14\nT\n27\n')

    def test_forwarding_and_keywords(self):
        source=PREFIX.replace('end module', '''subroutine forward(x,which)
class(shape),allocatable,intent(out)::x
integer,intent(in)::which
call make_shape(which=which,x=x)
end subroutine
end module''')
        self.check(source+'''program p
use allocatable_shapes
class(shape),allocatable::a,b
call forward(which=2,x=a)
call forward(b,1)
call inspect(a)
call inspect(b)
call change(a)
call inspect(a)
deallocate(a,b)
end program
''','F\nF\nT\n14\nT\n12\nT\n27\n')

    def test_function_inputs_allocated_and_unallocated(self):
        source=PREFIX.replace('end module', '''real function value(x)
class(shape),allocatable,intent(in)::x
value=-1
if(allocated(x)) value=x%area()
end function
end module''')
        self.check(source+'''program p
use allocatable_shapes
class(shape),allocatable::x
print *,value(x)
call make_shape(x,1)
print *,value(x)
x=rectangle(width=2.0,height=7.0)
print *,value(x)
deallocate(x)
print *,value(x)
end program
''','-1\nF\n12\n14\n-1\n')

    def test_descriptor_function_in_constructor(self):
        source=PREFIX.replace('end module', '''real function value(x)
class(shape),allocatable,intent(in)::x
value=x%area()
end function
end module''')
        self.check(source+'''program p
use allocatable_shapes
class(shape),allocatable::x,y
x=circle(radius=2.0)
y=circle(radius=value(x))
print *,y%area()
deallocate(x,y)
end program
''','432\n')

    def test_intent_in_rejects_mutations(self):
        for mutation in ['allocate(circle::x)','deallocate(x)','x=circle()',
                         'call change(x)']:
            source=PREFIX.replace('subroutine inspect(x)','subroutine forbidden(x)')
            source=source.replace('print *,allocated(x)\nif(allocated(x))print *,x%area()',mutation)
            with self.subTest(mutation=mutation):
                self.reject(source+'program p\nuse allocatable_shapes\nend program\n','INTENT(IN)')

    def test_nonallocatable_actual_rejected(self):
        self.reject(PREFIX+'''program p
use allocatable_shapes
type(circle)::x
call inspect(x)
end program
''','requires a polymorphic ALLOCATABLE actual')

    def test_different_declared_type_rejected(self):
        self.reject(PREFIX+'''program p
use allocatable_shapes
class(circle),allocatable::x
allocate(circle::x)
call inspect(x)
end program
''','must have the same declared type')

    def test_intent_out_can_return_unallocated(self):
        source=PREFIX.replace('end module', '''subroutine clear(x)
class(shape),allocatable,intent(out)::x
print *,allocated(x)
end subroutine
end module''')
        self.check(source+'''program p
use allocatable_shapes
class(shape),allocatable::x
x=circle()
call clear(x)
print *,allocated(x)
call clear(x)
print *,allocated(x)
end program
''','F\nF\nF\nF\n')

    def test_repeated_local_lifetimes(self):
        source=PREFIX.replace('end module', '''subroutine work(which)
integer,intent(in)::which
class(shape),allocatable::x
call make_shape(x,which)
call inspect(x)
end subroutine
end module''')
        self.check(source+'''program p
use allocatable_shapes
call work(1)
call work(2)
end program
''','F\nT\n12\nF\nT\n14\n')

    def test_separately_compiled_module(self):
        module,main=FIXTURE.split('program p',1)
        with tempfile.TemporaryDirectory(prefix='polymorphic-dummy-') as temporary:
            work=Path(temporary)
            (work/'m.f90').write_text(module)
            (work/'main.f90').write_text('program p'+main)
            for command in ([COMPILER,'-c','m.f90','-o','m.obj'],[COMPILER,'main.f90','m.obj','-o','main.exe']):
                result=subprocess.run(command,cwd=work,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
            result=subprocess.run([work/'main.exe'],capture_output=True,text=True,timeout=15)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.split(),['F','F','T','12.0','F','T','14.0','T','27.0'])

    def test_multiple_descriptors_and_default_intent(self):
        source=PREFIX.replace('end module', """subroutine swap(x,y)
class(shape),allocatable::x,y,temp
temp=x
x=y
y=temp
end subroutine
end module""")
        self.check(source+"""program p
use allocatable_shapes
class(shape),allocatable::x,y
x=circle(radius=3.0)
y=rectangle(width=2.0,height=7.0)
call swap(x,y)
print *,x%area(),y%area()
deallocate(x,y)
end program
""",'14 27\n')

    def test_owned_component_replacement(self):
        self.check("""module m
implicit none
type::record
integer::n=0
real,allocatable::data(:)
end type
contains
subroutine build(x,n)
class(record),allocatable,intent(out)::x
integer,intent(in)::n
allocate(record::x)
select type(x)
type is(record)
allocate(x%data(n))
x%data=real(n)
x%n=n
end select
end subroutine
subroutine inspect(x)
class(record),allocatable,intent(in)::x
select type(x)
type is(record)
print *,x%n,sum(x%data)
end select
end subroutine
end module
program p
use m
class(record),allocatable::x
call build(x,2)
call inspect(x)
call build(x,3)
call inspect(x)
deallocate(x)
end program
""",'2 4\n3 9\n')

    def test_pure_function_input(self):
        source=PREFIX.replace('real function','pure real function')
        source=source.replace('end module', """pure real function value(x)
class(shape),allocatable,intent(in)::x
value=-1
if(allocated(x))value=x%area()
end function
end module""")
        self.check(source+"""program p
use allocatable_shapes
class(shape),allocatable::x
print *,value(x)
x=circle()
print *,value(x)
deallocate(x)
end program
""",'-1\n12\n')

    def test_intent_in_cannot_pass_to_nonallocatable_output(self):
        source=PREFIX.replace('end module', """subroutine ordinary(x)
class(shape),intent(inout)::x
end subroutine
subroutine readonly(x)
class(shape),allocatable,intent(in)::x
call ordinary(x)
end subroutine
end module""")
        self.reject(source+'program p\nuse allocatable_shapes\nend program\n','INTENT(IN)')

    def test_optional_descriptor_diagnosed(self):
        self.reject(PREFIX.replace('class(shape),allocatable,intent(in)::x',
                                   'class(shape),allocatable,optional,intent(in)::x')+
                    'program p\nuse allocatable_shapes\nend program\n',
                    'OPTIONAL polymorphic ALLOCATABLE dummy')

    def test_mixed_intrinsic_components(self):
        self.check("""module m
implicit none
type::record
character(len=4)::label='none'
integer::n=1
logical::ready=.false.
complex::z
end type
contains
subroutine build(x)
class(record),allocatable,intent(out)::x
allocate(record::x)
select type(x)
type is(record)
x%label='done'
x%n=7
x%ready=.true.
x%z=(3.0,4.0)
end select
end subroutine
subroutine inspect(x)
class(record),allocatable,intent(in)::x
select type(x)
type is(record)
print *,x%label,x%n,x%ready,abs(x%z)
end select
end subroutine
end module
program p
use m
class(record),allocatable::x
call build(x)
call inspect(x)
call inspect(x)
deallocate(x)
end program
""",'done 7 T 5\ndone 7 T 5\n')

    def test_pure_polymorphic_intent_out_rejected(self):
        source=PREFIX.replace('subroutine make_shape(x,which)',
                              'pure subroutine make_shape(x,which)')
        self.reject(source+'program p\nuse allocatable_shapes\nend program\n',
                    'PURE procedure cannot have a polymorphic INTENT(OUT) dummy')

    def test_function_can_replace_allocation(self):
        source=PREFIX.replace('end module', """logical function create(x)
class(shape),allocatable,intent(out)::x
allocate(circle::x)
create=.true.
end function
end module""")
        self.check(source+"""program p
use allocatable_shapes
class(shape),allocatable::x
if(create(x))call inspect(x)
x=rectangle()
if(create(x))call inspect(x)
deallocate(x)
end program
""",'T\n12\nT\n12\n')

if __name__=='__main__':
    unittest.main()

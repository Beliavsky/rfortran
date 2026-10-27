"""Runtime scalar polymorphic ownership and dispatch."""
import unittest
import subprocess
import tempfile
from pathlib import Path
import test_deallocate
from test_record_read import ROOT, COMPILER

FIXTURE=(ROOT/'tests/programs/xpolymorphic_allocatable.f90').read_text(encoding='utf-8')
PREFIX=FIXTURE.split('program test_runtime_shapes')[0]

def program(body, declarations=''):
    return PREFIX+'program p\nuse runtime_shapes\nimplicit none\nclass(shape), allocatable :: x\n'+declarations+'\n'+body+'\nend program\n'

class PolymorphicAllocatable(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check(FIXTURE,'F\nT 12\n27\nT 12\n15\nT 12\n27\nT 12\n15\nF\n')

    def test_allocation_checks(self):
        for body,message in [
            ('deallocate(x)','deallocation of unallocated polymorphic object'),
            ('allocate(circle::x)\nallocate(rectangle::x)','allocation of already allocated polymorphic object'),
            ('print *,x%area()','use of unallocated polymorphic object')]:
            with self.subTest(body=body):
                self.check(program(body),error=message,reference=False)

    def test_abstract_allocation(self):
        self.reject(program('allocate(shape::x)'), 'cannot allocate ABSTRACT type')

    def test_unrelated_allocation(self):
        self.reject(program('allocate(other::x)','type :: other\ninteger :: n\nend type'),
                    'must extend declared type')

    def test_define_uninitialized_component(self):
        source=program("""allocate(circle::x)
select type(x)
type is(circle)
x%radius=4
print *,x%area()
end select
print *,x%area()
deallocate(x)""").replace('real :: radius=2.0','real :: radius')
        self.check(source,'48\n48\n')

    def test_class_procedure_forwarding(self):
        self.check(program("""allocate(circle::x)
call show(x)
deallocate(x)
allocate(rectangle::x)
call show(x)
contains
subroutine show(s)
class(shape),intent(in)::s
print *,s%area()
end subroutine"""),'12\n12\n')

    def test_two_runtime_objects(self):
        self.check(program("""allocate(circle::x, y)
print *,x%area()+y%area()
deallocate(x,y)
allocate(rectangle::x)
allocate(circle::y)
print *,x%area()+y%area()
deallocate(y,x)""",'class(shape),allocatable::y'),'24\n24\n')

    def test_loop_condition_and_exit(self):
        self.check(program("""allocate(circle::x)
i=0
do while(x%area()<100)
i=i+1
select type(x)
type is(circle)
x%radius=x%radius+1
end select
if(x%area()>40) exit
end do
print *,i,x%area()
deallocate(x)""",'integer::i'),'2 48\n')

    def test_guard_priority_and_default(self):
        self.check(program("""allocate(circle::x)
select type(s=>x)
class is(shape)
print *,1
type is(circle)
print *,2
end select
select type(s=>x)
type is(rectangle)
print *,3
class default
print *,4
end select
deallocate(x)"""),'2\n4\n')

    def test_local_lifetime_and_default_initialization(self):
        self.check(PREFIX+"""program p
use runtime_shapes
call work(1)
call work(2)
contains
subroutine work(i)
integer,intent(in)::i
class(shape),allocatable::x
print *,allocated(x)
if(i==1) then
allocate(circle::x)
else
allocate(rectangle::x)
end if
print *,x%area()
end subroutine
end program
""",'F\n12\nF\n12\n')

    def test_capture_runtime_array_bounds(self):
        self.check(PREFIX+"""program p
use runtime_shapes
call work(3)
contains
subroutine work(n)
integer,intent(in)::n
class(shape),allocatable::x
real::a(n)
allocate(circle::x)
a=1
a(1)=x%area()
print *,sum(a)
end subroutine
end program
""",'14\n')

    def test_inherited_components_and_overridden_subroutine(self):
        prefix=(ROOT/'tests/programs/xdynamic_dispatch.f90').read_text().split('program test_dynamic_dispatch')[0]
        self.check(prefix+"""program p
use dynamic_counters
class(counter),allocatable::x
allocate(scaled_counter::x)
x%value=3
call show(x)
print *,x%value
call x%report()
deallocate(x)
allocate(counter::x)
print *,x%value,x%get()
deallocate(x)
end program
""",'6\n14\n7\n14\n1 1\n')

    def test_plain_record_element_borrow_preserves_alias(self):
        self.check("""module m
implicit none
type::t
integer::a,b
end type
contains
subroutine set_a(x)
type(t),intent(inout)::x
x%a=7
end subroutine
end module
program p
use m
type(t)::a(1)
call set_a(a(1))
print *,a(1)%a
end program
""",'7\n')

    def test_unsupported_storage_diagnostics(self):
        self.reject(program('allocate(circle::x)').replace('allocatable :: x','allocatable,save :: x'),
                    'SAVED or POINTER runtime polymorphic objects')
        self.reject(program('allocate(circle::x)').replace('allocatable :: x','allocatable :: x(:)'),
                    'runtime polymorphic ALLOCATABLE arrays')

    def test_captured_optional_argument_presence(self):
        self.check(PREFIX+"""program p
use runtime_shapes
call work()
call work(7)
contains
subroutine work(n)
integer,optional,intent(in)::n
class(shape),allocatable::x
allocate(circle::x)
print *,present(n),x%area()
end subroutine
end program
""",'F 12\nT 12\n')

    def test_separately_compiled_type_module(self):
        module,main=FIXTURE.split('program test_runtime_shapes',1)
        with tempfile.TemporaryDirectory(prefix='runtime-polymorphic-module-') as temporary:
            work=Path(temporary)
            (work/'m.f90').write_text(module,encoding='utf-8')
            (work/'main.f90').write_text('program test_runtime_shapes'+main,encoding='utf-8')
            for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                            [COMPILER,'main.f90','m.obj','-o','main.exe']):
                result=subprocess.run(command,cwd=work,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
            result=subprocess.run([work/'main.exe'],capture_output=True,text=True,timeout=15)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.split(),['F','T','12.0','27.0','T','12.0','15.0',
                                                   'T','12.0','27.0','T','12.0','15.0','F'])

    def test_source_and_mold(self):
        self.check(program("""c%radius=4
allocate(x,source=c)
print *,x%area()
allocate(y,mold=x)
print *,y%area()
deallocate(x)
allocate(x,source=y)
print *,x%area()
deallocate(x,y)""",'type(circle)::c\nclass(shape),allocatable::y'),'48\n12\n12\n')

    def test_assignment_reallocates_dynamic_type(self):
        self.check(program("""x=circle(radius=4.0)
print *,x%area()
y=x
select type(y)
type is(circle)
y%radius=5
end select
print *,x%area(),y%area()
x=rectangle(width=2.0,height=7.0)
print *,x%area()
y=x
print *,y%area()
x=x
print *,x%area()
deallocate(x,y)""",'class(shape),allocatable::y'),'48\n48 75\n14\n14\n14\n')

    def test_mold_does_not_read_undefined_components(self):
        source=program("""allocate(x,mold=c)
select type(x)
type is(circle)
x%radius=3
end select
print *,x%area()
deallocate(x)""",'type(circle)::c').replace('real :: radius=2.0','real :: radius')
        self.check(source,'27\n')

    def test_source_constructor_and_multiple_objects(self):
        self.check(program("""allocate(x,y,source=circle(radius=3.0))
print *,x%area(),y%area()
deallocate(x,y)""",'class(shape),allocatable::y'),'27 27\n')

    def test_unallocated_assignment_source(self):
        self.check(program('x=y','class(shape),allocatable::y'),
                   error='unallocated polymorphic SOURCE or MOLD',reference=False)

    def test_invalid_allocation_options(self):
        self.reject(program('allocate(x,source=circle(),mold=circle())'),
                    'both SOURCE= and MOLD=')
        self.reject(program('allocate(x,source=circle(),source=circle())'),
                    'duplicate SOURCE=')

    def test_source_expression_evaluated_once(self):
        source=program("""allocate(x,y,source=circle(radius=next_radius()))
print *,calls,x%area(),y%area()
deallocate(x,y)""",'class(shape),allocatable::y')
        source=source.replace('type, abstract :: shape','integer::calls=0\ntype, abstract :: shape')
        source=source.replace('real function circle_area(self)',"""real function next_radius()
calls=calls+1
next_radius=3
end function
real function circle_area(self)""")
        self.check(source,'1 27 27\n')

    def test_assignment_copies_same_type_and_is_independent(self):
        self.check(program("""c%radius=3
x=c
y=x
c%radius=4
x=c
print *,x%area(),y%area()
deallocate(x,y)""",'type(circle)::c\nclass(shape),allocatable::y'),'48 27\n')

    def test_unallocated_concrete_mold(self):
        prefix=(ROOT/'tests/programs/xdynamic_dispatch.f90').read_text().split('program test_dynamic_dispatch')[0]
        self.check(prefix+"""program p
use dynamic_counters
class(counter),allocatable::x,y
allocate(x,mold=y)
print *,allocated(x),allocated(y),x%get()
deallocate(x)
end program
""",'T F 1\n')

    def test_owned_allocatable_component_copy(self):
        self.check("""module m
implicit none
type::record
integer::id=0
real,allocatable::data(:)
end type
end module
program p
use m
type(record)::a
class(record),allocatable::x,y
allocate(a%data(2))
a%data=[1.0,2.0]
a%id=7
allocate(x,source=a)
y=x
a%data=9
select type(x)
type is(record)
x%data(1)=4
end select
select type(y)
type is(record)
print *,y%id,y%data
end select
deallocate(x,y)
end program
""",'7 1 2\n')

    def test_constructor_uses_original_dynamic_value(self):
        self.check(program("""x=circle(radius=2.0)
x=rectangle(width=x%area(),height=2.0)
print *,x%area()
allocate(y,source=circle(radius=x%area()))
print *,y%area()
deallocate(x,y)""",'class(shape),allocatable::y'),'24\n1728\n')

if __name__=='__main__':
    unittest.main()

"""Preserve allocatable descriptors through nested type-bound calls."""
import unittest
from pathlib import Path
import test_deallocate as helpers
import test_module_compilation

FIXTURE = (Path(__file__).parent / "programs/xallocatable_component_forward.f90").read_text()
WRITABLE = (Path(__file__).parent / "programs/xwritable_component_function.f90").read_text()

class AllocatableComponentForward(unittest.TestCase):
    check = helpers.Deallocate.check
    reject = helpers.Deallocate.reject

    def test_real_matrix(self):
        self.check(FIXTURE, "2 3 3\n")

    def test_integer_matrix_with_lower_bounds(self):
        source = FIXTURE.replace("real, allocatable", "integer, allocatable")
        source = source.replace(" > 0.0", " > 0")
        source = source.replace("x%values(2,3)", "x%values(-1:0,4:6)")
        source = source.replace("[1.,-1.,2.,-2.,3.,-3.]", "[1,-1,2,-2,3,-3]")
        self.check(source, "2 3 3\n")

    def test_rank_three(self):
        source = FIXTURE.replace("(:,:)", "(:,:,:)")
        source = source.replace("x%values(2,3)", "x%values(2,1,3)")
        source = source.replace("[2,3]", "[2,1,3]")
        self.check(source, "2 1 3 3\n")

    def test_logical_component(self):
        source = FIXTURE.replace("real, allocatable", "logical, allocatable")
        source = source.replace("self%values > 0.0", "self%values")
        source = source.replace("[1.,-1.,2.,-2.,3.,-3.]", "[.true.,.false.,.true.,.false.,.true.,.false.]")
        self.check(source, "2 3 3\n")

    def test_nested_write_updates_original_storage(self):
        source = FIXTURE.replace("intent(in) :: self", "intent(inout) :: self")
        source = source.replace("mask = self%values > 0.0",
                                "self%values = self%values + 10.0\nmask = self%values > 0.0")
        source = source.replace("call x%inspect()", "call x%inspect()\nprint *, sum(x%values)")
        self.check(source, "2 3 6\n60.0\n")

    def test_writable_runtime_bounds(self):
        self.check(WRITABLE, '2 3 6\n-1 4 0 6 60.0\n', data='3\n')

    def test_writable_integer_component(self):
        source=WRITABLE.replace('real, allocatable', 'integer, allocatable')
        source=source.replace('10.0','10').replace('0.0','0')
        source=source.replace('[1.,-1.,2.,-2.,3.,-3.]','[1,-1,2,-2,3,-3]')
        self.check(source,'2 3 6\n-1 4 0 6 60\n',data='3\n')

    def test_writable_rank_three_component(self):
        source=WRITABLE.replace('(:,:)', '(:,:,:)')
        source=source.replace('(-1:0,4:n+3)', '(-1:0,4:4,7:n+6)')
        source=source.replace('[2,n]', '[2,1,n]')
        self.check(source,'2 1 3 6\n-1 4 7 0 4 9 60.0\n',data='3\n')

    def test_writable_logical_component(self):
        source=WRITABLE.replace('real, allocatable','logical, allocatable')
        source=source.replace('self%values = self%values + 10.0','self%values = .not. self%values')
        source=source.replace('self%values > 0.0','self%values')
        source=source.replace('[1.,-1.,2.,-2.,3.,-3.]','[.true.,.false.,.true.,.false.,.true.,.false.]')
        source=source.replace('sum(x%values)', 'count(x%values)')
        self.check(source,'2 3 3\n-1 4 0 6 3\n',data='3\n')

    def test_ordinary_writable_function_dummy(self):
        for intent in ('inout','out'):
            with self.subTest(intent=intent):
                self.check(f'''implicit none
real,allocatable::x(:,:)
logical,allocatable::mask(:,:)
integer::n
read *,n
allocate(x(-1:0,4:n+3))
x=1.0
mask=change(x)
print *,shape(mask),count(mask),sum(x),lbound(x)
contains
function change(a) result(mask)
real,intent({intent})::a(:,:)
logical,allocatable::mask(:,:)
a=5.0
mask=a>0.0
end function
end
''','2 3 6 30.0 -1 4\n',data='3\n')

    def test_overridden_writable_binding(self):
        source=WRITABLE.replace('end type\ncontains','''end type
type,extends(table)::extended_table
contains
 procedure::positive=>extended_positive
end type
contains''',1)
        source=source.replace('end module','''function extended_positive(self) result(mask)
class(extended_table),intent(inout)::self
logical,allocatable::mask(:,:)
self%values=self%values+20.0
mask=self%values>0.0
end function
end module''')
        source=source.replace('type(table) :: x','type(extended_table) :: x')
        self.check(source,'2 3 6\n-1 4 0 6 120.0\n',data='3\n')

    def test_writable_separate_module(self):
        helper=test_module_compilation.ModuleCompilation()
        helper.setUp()
        self.addCleanup(helper.doCleanups)
        module,main=WRITABLE.split('program main',1)
        source=helper.source('owners',module)
        helper.success('-c',source)
        source.unlink()
        main=helper.source('main','program main'+main.replace('read *, n','n=3'))
        helper.success('-c',main)
        helper.run_objects(['owners','main'],'2 3 6\n-1 4 0 6 60.0')

    def test_writable_runtime_expression_rejected(self):
        self.reject('''implicit none
real,allocatable::x(:,:)
logical,allocatable::mask(:,:)
integer::n
read *,n
allocate(x(n,n))
x=1.0
mask=change(x+1.0)
contains
function change(a) result(mask)
real,intent(inout)::a(:,:)
logical,allocatable::mask(:,:)
a=5.0
mask=a>0.0
end function
end
''','writable function dummy')

    def test_runtime_output_discards_previous_initialization(self):
        source='''implicit none
real,allocatable::x(:,:)
logical,allocatable::mask(:,:)
integer::n
read *,n
allocate(x(n,n))
x=1.0
mask=change(x)
contains
function change(a) result(mask)
real,intent(out)::a(:,:)
logical,allocatable::mask(:,:)
a(1,1)=5.0
mask=a>0.0
end function
end
'''
        with self.subTest(read='inside function'):
            self.check(source,error='uninitialized',data='3\n')
        with self.subTest(read='after return'):
            after=source.replace('mask=change(x)','mask=change(x)\nprint *,x(2,2)')
            after=after.replace('mask=a>0.0','allocate(mask(size(a,1),size(a,2)))\nmask=.true.')
            self.check(after,error='uninitialized',data='3\n')

    def test_empty_output_array(self):
        self.check('''implicit none
real,allocatable::x(:,:)
logical,allocatable::mask(:,:)
integer::n
read *,n
allocate(x(n,n))
mask=change(x)
print *,size(x),size(mask)
contains
function change(a) result(mask)
real,intent(out)::a(:,:)
logical,allocatable::mask(:,:)
a=5.0
mask=a>0.0
end function
end
''','0 0\n',data='0\n')

    def test_unallocated_component(self):
        source = FIXTURE.replace('if (.not. allocated(self%values)) error stop "values not allocated"',
                                 'print *, allocated(self%values)\nallocate(mask(0,0))\nreturn')
        source = source.replace("allocate(x%values(2,3))", "")
        source = source.replace("x%values = reshape([1.,-1.,2.,-2.,3.,-3.],[2,3])", "")
        self.check(source, "F\n0 0 0\n")

    def test_expression_is_not_allocatable(self):
        self.reject("""implicit none
integer, allocatable :: a(:)
allocate(a(3))
a=1
call take(a + 1)
contains
subroutine take(x)
integer, allocatable, intent(in) :: x(:)
print *, size(x)
end subroutine
end
""", "requires an ALLOCATABLE variable")

if __name__ == "__main__":
    unittest.main()

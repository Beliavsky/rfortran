"""ALLOCATABLE enumeration arrays in interpreter and native O0/O2 builds."""
from pathlib import Path
import subprocess
import tempfile
import unittest

import test_enumerations as enums
import test_fortran2023 as support
from test_record_read import ROOT, build_commands


class EnumerationAllocatables(unittest.TestCase):
    check = support.Fortran2023.check
    reject = support.Fortran2023.reject

    def test_example(self):
        self.check((ROOT/'tests/programs/xenumeration_allocatable.f90').read_text(),
                   ' F\n 3 3 3 0 0 0\n F T 3\n', data='3\n')

    def test_assignment_reallocation_source_and_mold(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour),allocatable::a(:),b(:),c(:),d(:)
integer::status
character(80)::message
a=[red,green]
print *,allocated(a),size(a),int(a)
a=[orange]
allocate(b,source=a)
print *,size(a),int(a),int(b)
a=[colour ::]
print *,allocated(a),size(a)
allocate(c,mold=b)
c=green
print *,int(c)
allocate(d(2),source=red)
print *,int(d)
allocate(d(3),stat=status,errmsg=message)
print *,status>0,len_trim(message)>0
deallocate(d)
allocate(d(2),mold=green)
d=orange
print *,int(d)
deallocate(a,b,c,d)
deallocate(d,stat=status)
print *,status>0
end
''',' T 2 1 3\n 1 2 2\n T 0\n 3\n 1 1\n T T\n 2 2\n T\n')

    def test_multidimensional_and_rank_fifteen(self):
        for rank in (2,3,15):
            dims=','.join(['n']+['1']*(rank-1))
            deferred=','.join([':']*rank)
            with self.subTest(rank=rank):
                self.check('implicit none\n'+enums.ENUM+f'''type(colour),allocatable::a({deferred})
integer,allocatable::status({deferred})
integer::n
read *,n
allocate(a({dims}),status({dims}))
a=orange
a=next(a,status)
print *,size(a),sum(int(a)),sum(status)
deallocate(a,status)
print *,allocated(a)
end
''',' 3 9 0\n F\n',data='3\n')

    def test_move_alloc_keywords_and_bounds(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour),allocatable::a(:),b(:)
allocate(a(-2:0),b(1))
a=[red,orange,green]
b=red
call move_alloc(to=b,from=a)
print *,allocated(a),allocated(b),lbound(b),ubound(b),int(b)
call move_alloc(a,b)
print *,allocated(a),allocated(b)
end
''',' F T -2 0 1 2 3\n F F\n')

    def test_allocatable_dummy_arguments(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour),allocatable::a(:)
a=[red,orange]
call make(a,3)
print *,size(a),int(a)
call grow(a)
print *,size(a),int(a)
contains
subroutine make(x,n)
type(colour),allocatable,intent(out)::x(:)
integer,intent(in)::n
allocate(x(n))
x=green
end subroutine
subroutine grow(x)
type(colour),allocatable,intent(inout)::x(:)
x=[x,red]
end subroutine
end
''',' 3 3 3 3\n 4 3 3 3 1\n')

    def test_allocatable_function_result(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour),allocatable::a(:)
a=copy([red,green])
print *,int(a)
contains
function copy(x) result(y)
type(colour),intent(in)::x(:)
type(colour),allocatable::y(:)
y=x
end function
end
''',' 1 3\n')

    def test_module_storage(self):
        self.check('module m\nimplicit none\n'+enums.ENUM+'''type(colour),allocatable::lights(:)
contains
subroutine setup(n)
integer,intent(in)::n
allocate(lights(n))
lights=orange
end subroutine
end module
use m
implicit none
call setup(3)
print *,int(lights)
deallocate(lights)
print *,allocated(lights)
end
''',' 2 2 2\n F\n')

    def test_derived_type_components(self):
        self.check('implicit none\n'+enums.ENUM+'''type box
type(colour),allocatable::a(:)
end type
type(box)::x,y
allocate(x%a(2))
x%a=[red,green]
call move_alloc(x%a,y%a)
print *,allocated(x%a),int(y%a)
deallocate(y%a)
print *,allocated(y%a)
end
''',' F 1 3\n F\n')

    def test_nominal_type_checks_before_storage_erasure(self):
        prefix='implicit none\n'+enums.ENUM+'''enumeration type :: season
enumerator :: spring,summer
end enumeration type
type(colour),allocatable::a(:)
type(season),allocatable::b(:)
integer,allocatable::numbers(:)
'''
        for statement in ('a=[1,2]', 'allocate(a(2),source=1)',
                          'allocate(a(2),source=spring)', 'allocate(a(2),mold=spring)',
                          'allocate(numbers(2),source=red)', 'call move_alloc(a,b)',
                          'call move_alloc(a,numbers)'):
            with self.subTest(statement=statement):
                self.reject(prefix+statement+'\nend\n','same enumeration type')
        self.reject(prefix+'allocate(a(red))\nend\n','bounds must be INTEGER')

    def test_allocatable_attribute_is_required(self):
        prefix='implicit none\n'+enums.ENUM+'type(colour)::a(2)\n'
        self.reject(prefix+'allocate(a(2))\nend\n','ALLOCATABLE attribute')
        self.reject(prefix+'print *,allocated(a)\nend\n','whole ALLOCATABLE')
        for actual in ('a','a(1:2)'):
            with self.subTest(actual=actual):
                self.reject(prefix+f'''call f({actual})
contains
subroutine f(x)
type(colour),allocatable::x(:)
end subroutine
end
''','whole ALLOCATABLE')

    def test_intent_in_cannot_be_allocated(self):
        self.reject('module m\n'+enums.ENUM+'''contains
subroutine f(x)
type(colour),allocatable,intent(in)::x(:)
allocate(x(2))
end subroutine
end module
end
''','INTENT(IN)')

    def test_uninitialized_allocated_element(self):
        source='implicit none\n'+enums.ENUM+'''type(colour),allocatable::a(:)
integer::n
read *,n
allocate(a(n))
a(1)=red
print *,int(a)
end
'''
        with tempfile.TemporaryDirectory() as tmp:
            work=Path(tmp);path=work/'unset.f90';path.write_text(source)
            for command in build_commands(path,work,cwd=work):
                p=subprocess.run(command,cwd=work,input='3\n',capture_output=True,text=True,timeout=30)
                self.assertNotEqual(p.returncode,0,p.stdout+p.stderr)
                self.assertIn('uninitialized',p.stderr)
                self.assertIn('element 2',p.stderr)

    def test_move_alloc_preserves_uninitialized_elements(self):
        source='implicit none\n'+enums.ENUM+'''type(colour),allocatable::a(:),b(:)
integer::n
read *,n
allocate(a(n))
a(1)=red
call move_alloc(a,b)
print *,int(b)
end
'''
        with tempfile.TemporaryDirectory() as tmp:
            work=Path(tmp);path=work/'move_unset.f90';path.write_text(source)
            for command in build_commands(path,work,cwd=work):
                p=subprocess.run(command,cwd=work,input='3\n',capture_output=True,text=True,timeout=30)
                self.assertNotEqual(p.returncode,0,p.stdout+p.stderr)
                self.assertIn('uninitialized variable `b`: element 2',p.stderr)

    def test_zero_extent_and_pure_move(self):
        self.check('implicit none\n'+enums.ENUM+'''type(colour),allocatable::a(:),b(:)
allocate(a(0))
call relocate(a,b)
print *,allocated(a),allocated(b),size(b),sum(int(b))
contains
pure subroutine relocate(x,y)
type(colour),allocatable,intent(inout)::x(:),y(:)
call move_alloc(x,y)
end subroutine
end
''',' F T 0 0\n')

    def test_move_through_dummy_preserves_initialization(self):
        source='implicit none\n'+enums.ENUM+'''type(colour),allocatable::a(:),b(:)
integer::n
read *,n
allocate(a(n))
a(1)=red
call relocate(a,b)
print *,int(b)
contains
pure subroutine relocate(x,y)
type(colour),allocatable,intent(inout)::x(:),y(:)
call move_alloc(x,y)
end subroutine
end
'''
        with tempfile.TemporaryDirectory() as tmp:
            work=Path(tmp);path=work/'dummy_unset.f90';path.write_text(source)
            for command in build_commands(path,work,cwd=work):
                p=subprocess.run(command,cwd=work,input='3\n',capture_output=True,text=True,timeout=30)
                self.assertNotEqual(p.returncode,0,p.stdout+p.stderr)
                self.assertIn('uninitialized variable `b`: element 2',p.stderr)


if __name__=='__main__':
    unittest.main()

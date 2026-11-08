"""Dummy-local lower bounds and actual extents across array call binding."""
import unittest
import subprocess
import tempfile
from pathlib import Path
from test_record_read import ROOT, COMPILER
import test_fortran2023
import test_concurrent_parallel


class AssumedShapeLowerBounds(unittest.TestCase):
    check = test_fortran2023.Fortran2023.check
    reject = test_fortran2023.Fortran2023.reject
    build_run = test_concurrent_parallel.ParallelConcurrent.build_run

    def all_modes(self, source, parallel=False):
        self.check(source.replace('NITER','3'),' PASS\n')
        if parallel:
            self.build_run(source.replace('NITER','9001'),'PASS')

    def test_original_reproducer(self):
        self.all_modes((ROOT/'tests/programs/xassumed_shape_explicit_lower_bound_repro.f90').read_text())

    def test_rank_one_sections_and_inquiries(self):
        self.all_modes('''implicit none
integer::x(-3:3)
x=10
call update(x(-3:3:2))
if(any(x(-3:3:2)/=[21,10,10,24])) stop 1
if(any(x(-2:2:2)/=10)) stop 2
print *,"PASS"
contains
subroutine update(a)
integer,intent(inout)::a(-2:)
if(any(lbound(a)/=[-2]).or.any(ubound(a)/=[1])) stop 3
if(any(shape(a)/=[4]).or.size(a)/=4) stop 4
a(-2)=21
a(1)=24
end subroutine
end
''')

    def test_rank_two_strided_section(self):
        self.all_modes('''implicit none
integer::x(-1:3,2:7)
x=10
call update(x(-1:3:2,3:7:2))
if(x(-1,3)/=42.or.x(3,7)/=73) stop 1
if(count(x/=10)/=2) stop 2
print *,"PASS"
contains
subroutine update(a)
integer,intent(inout)::a(-2:,4:)
if(any(lbound(a)/=[-2,4]).or.any(ubound(a)/=[0,6])) stop 3
if(any(shape(a)/=[3,3]).or.size(a)/=9) stop 4
a(-2,4)=42
a(0,6)=73
end subroutine
end
''')

    def test_runtime_bound_is_evaluated_at_entry(self):
        self.all_modes('''implicit none
integer::x(3),lo
x=10
lo=-2
call update(x,lo)
if(lo/=8.or.any(x/=[7,10,10])) stop 1
print *,"PASS"
contains
subroutine update(a,lo)
integer,intent(inout)::lo
integer,intent(inout)::a(lo:)
if(lbound(a,1)/=lo) stop 2
lo=lo+10
if(lbound(a,1)/=-2.or.ubound(a,1)/=0) stop 3
a(-2)=7
end subroutine
end
''')

    def test_scalar_result_function(self):
        self.all_modes('''implicit none
integer::x(3)=[3,4,5]
if(first(x)/=3) stop 1
print *,"PASS"
contains
pure integer function first(a) result(n)
integer,intent(in)::a(-2:)
n=a(-2)
end function
end
''')

    def test_bound_dependency_uses_declaration_order(self):
        self.all_modes('''implicit none
integer::x(3),y(2)
x=10
y=20
call update(y,x)
if(x(1)/=7.or.y(1)/=8) stop 1
print *,"PASS"
contains
subroutine update(b,a)
integer,intent(inout)::a(-2:)
integer,intent(inout)::b(lbound(a,1):)
if(lbound(b,1)/=-2.or.ubound(b,1)/=-1) stop 2
a(-2)=7
b(-2)=8
end subroutine
end
''')

    def test_bound_expression_uses_actual_extent(self):
        self.all_modes('''implicit none
integer,allocatable::x(:)
integer::y(2),n
n=3
allocate(x(n))
x=10
y=20
call update(y,x)
if(y(1)/=8) stop 1
print *,"PASS"
contains
subroutine update(b,a)
integer,intent(in)::a(:)
integer,intent(inout)::b(1-size(a):)
if(lbound(b,1)/=-2.or.ubound(b,1)/=-1) stop 2
b(-2)=8
end subroutine
end
''')

    def test_reshape_uses_dummy_shape(self):
        source='''implicit none
integer::x(2,2)
call update(x)
if(any(x/=reshape([1,2,3,4],[2,2]))) stop 1
print *,"PASS"
contains
subroutine update(a)
integer,intent(out)::a(-2:,4:)
a=reshape([1,2,3,4],shape(a))
end subroutine
end
'''
        for dims in ['-2:,4:', ':,:']:
            with self.subTest(dims=dims):
                self.all_modes(source.replace('-2:,4:',dims))

    def test_separately_compiled_module(self):
        with tempfile.TemporaryDirectory(prefix='rfort-lower-module-') as tmp:
            root=Path(tmp);module=root/'m.f90';main=root/'main.f90'
            module.write_text('''module m
contains
subroutine update(a)
integer,intent(inout)::a(-2:,4:)
a(-2,4)=42
end subroutine
end module
''')
            main.write_text('''use m,only:update
implicit none
integer::x(2,2)
x=10
call update(x)
if(x(1,1)/=42.or.count(x/=10)/=1) stop 1
print *,"PASS"
end
''')
            obj=root/'m.obj';exe=root/'main.exe'
            for command in [[str(COMPILER),'-c',str(module),'-J',tmp,'-o',str(obj)],
                            [str(COMPILER),str(main),str(obj),'-I',tmp,'-o',str(exe)]]:
                result=subprocess.run(command,cwd=tmp,capture_output=True,text=True,timeout=90)
                self.assertEqual(result.returncode,0,result.stderr)
            result=subprocess.run([str(exe)],capture_output=True,text=True,timeout=30)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.split(),['PASS'])

    def test_allocated_actual_bounds_are_preserved(self):
        self.all_modes('''implicit none
integer,allocatable::x(:)
integer::n
do n=2,4
 allocate(x(-7:-7+n-1))
 x=10
 call update(x)
 if(any(x/=11)) stop 1
 if(lbound(x,1)/=-7.or.ubound(x,1)/=-7+n-1) stop 2
 deallocate(x)
end do
print *,"PASS"
contains
subroutine update(a)
integer,intent(inout)::a(-2:)
a=a+1
end subroutine
end
''')

    def test_negative_stride_actual(self):
        self.all_modes('''implicit none
integer::x(6)=[1,2,3,4,5,6]
call update(x(6:2:-2))
if(any(x/=[1,9,3,4,5,8])) stop 1
print *,"PASS"
contains
subroutine update(a)
integer,intent(inout)::a(-2:)
if(a(-2)/=6.or.a(0)/=2) stop 2
a(-2)=8
a(0)=9
end subroutine
end
''')

    def test_subscript_checks_use_dummy_bounds(self):
        source='''integer::x(2)
x=10
call update(x)
contains
subroutine update(a)
integer,intent(inout)::a(-2:)
a(-3)=7
end subroutine
end
'''
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'case.f90';path.write_text(source)
            commands=[[str(COMPILER),'run',str(path)]]
            for opt in ('0','2'):
                exe=Path(tmp)/f'case{opt}.exe'
                result=subprocess.run([str(COMPILER),'-O',opt,str(path),'-o',str(exe)],cwd=tmp,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                commands.append([str(exe)])
            for command in commands:
                result=subprocess.run(command,cwd=tmp,capture_output=True,text=True,timeout=30)
                self.assertNotEqual(result.returncode,0)
                self.assertIn('subscript outside',result.stderr)

    def test_intrinsic_element_types(self):
        for kind,initial,value in [
            ('integer(kind=8)','10_8','7_8'),
            ('real(kind=8)','10.0_8','7.0_8'),
            ('logical','.false.','.true.'),
            ('complex(kind=8)','cmplx(1.0_8,2.0_8,kind=8)','cmplx(3.0_8,4.0_8,kind=8)'),
            ('character(len=3)','"old"','"new"'),
        ]:
            with self.subTest(kind=kind):
                op='.neqv.' if kind=='logical' else '/='
                self.all_modes(f'''implicit none
{kind}::x(2)
x={initial}
call update(x)
if(x(1){op}{value}) stop 1
if(x(2){op}{initial}) stop 2
print *,"PASS"
contains
subroutine update(a)
{kind},intent(inout)::a(-1:)
a(-1)={value}
end subroutine
end
''')

    def test_derived_type_elements(self):
        self.all_modes('''implicit none
type::item
 integer::n
end type
type(item)::x(2)
x%n=1
call update(x)
if(any(x%n/=[7,1])) stop 1
print *,"PASS"
contains
subroutine update(a)
type(item),intent(inout)::a(-1:)
a(-1)%n=7
end subroutine
end
''')

    def test_assumed_shape_does_not_use_sequence_association(self):
        self.reject('''integer::x(2,2)
call update(x)
contains
subroutine update(a)
integer,intent(out)::a(-2:)
a=0
end subroutine
end
''','type or rank mismatch')

    def test_empty_dimension_bounds(self):
        self.all_modes('''implicit none
integer::x(0,2)
call update(x)
print *,"PASS"
contains
subroutine update(a)
integer,intent(inout)::a(-2:,4:)
if(any(shape(a)/=[0,2]).or.size(a)/=0) stop 1
if(any(lbound(a)/=[1,4]).or.any(ubound(a)/=[0,5])) stop 2
a=7
end subroutine
end
''')

    def test_forwarding_rebases_each_dummy_independently(self):
        self.all_modes('''implicit none
integer::x(3)
x=10
call outer(x)
if(any(x/=[55,10,10])) stop 1
print *,"PASS"
contains
subroutine outer(a)
integer,intent(inout)::a(-2:)
call inner(a)
if(lbound(a,1)/=-2.or.ubound(a,1)/=0) stop 2
end subroutine
subroutine inner(a)
integer,intent(inout)::a(5:)
if(lbound(a,1)/=5.or.ubound(a,1)/=7) stop 3
a(5)=55
end subroutine
end
''')

    def test_rank_one_and_two_parallel_reductions(self):
        for dims, declaration in [('3','-2:'),('2,3','-2:,4:')]:
            with self.subTest(dims=dims):
                self.all_modes(f'''implicit none
integer::x({dims})
x=10
call accumulate(x)
if(any(x/=10+NITER)) stop 1
print *,"PASS"
contains
subroutine accumulate(a)
integer,intent(inout)::a({declaration})
integer::i
do concurrent(i=1:NITER) reduce(+:a)
 a=a+1
end do
end subroutine
end
''',parallel=True)

    def test_parallel_pure_output_call(self):
        self.all_modes('''implicit none
integer::a(3,NITER),i
do concurrent(i=1:NITER)
 call fill(a(:,i),i)
end do
do i=1,NITER
 if(any(a(:,i)/=i)) stop 1
end do
print *,"PASS"
contains
pure subroutine fill(a,value)
integer,intent(out)::a(-2:)
integer,intent(in)::value
a(-2:0)=value
end subroutine
end
''',parallel=True)

    def test_rank_fifteen(self):
        self.all_modes(f'''implicit none
integer::x({','.join(['1']*15)})
x=10
call update(x)
if(any(x/=11)) stop 1
print *,"PASS"
contains
subroutine update(a)
integer,intent(inout)::a({','.join(['-2:']*15)})
if(any(shape(a)/=1).or.any(lbound(a)/=-2).or.any(ubound(a)/=-2)) stop 2
a({','.join(['-2']*15)})=11
end subroutine
end
''')


if __name__=='__main__':
    unittest.main()

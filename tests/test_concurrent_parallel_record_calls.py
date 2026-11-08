"""Audited PURE/SIMPLE calls with bounded private record actuals."""
import subprocess
import tempfile
import unittest
from pathlib import Path
from test_record_read import ROOT, COMPILER
import test_concurrent_parallel_local_records as local_records

class ParallelRecordCalls(unittest.TestCase):
    check_source=local_records.ParallelLocalRecords.check_source
    build_run=local_records.ParallelLocalRecords.build_run
    reject=local_records.ParallelLocalRecords.reject

    @staticmethod
    def example():
        return (ROOT/'tests/programs/xconcurrent_parallel_record_calls.f90').read_text()

    def test_inout(self):
        self.check_source(self.example())

    def test_serial(self):
        self.build_run(self.example(),'PASS',parallel=False,threads=(1,))

    def test_interpreter(self):
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'case.f90';path.write_text(self.example().replace('n=9000','n=3'))
            result=subprocess.run([str(COMPILER),'run',str(path)],cwd=tmp,capture_output=True,text=True,timeout=30)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.split(),['PASS'])

    def test_simple(self):
        self.check_source(self.example().replace('pure subroutine','simple subroutine'))

    def test_checks_off(self):
        self.build_run(self.example(),'PASS',checks=False)

    def test_wider_component_kinds(self):
        self.check_source(self.example().replace('complex ::','complex(8) ::').replace('integer :: count','integer(8) :: count'))

    def test_multidimensional_fields(self):
        for rank in (2,3):
            with self.subTest(rank=rank):
                dims='-1:1'+',1:1'*(rank-1)
                shape='3'+',1'*(rank-1)
                source=self.example().replace('position(-1:1), velocity(-1:1)',f'position({dims}), velocity({dims})').replace('[1.,2.,3.]',f'reshape([1.,2.,3.],[{shape}])')
                self.check_source(source)

    def test_local_init(self):
        self.check_source(self.example().replace('work%count=-99','work=particles(1)\nwork%count=-99').replace('local(work)','local_init(work)'))

    def test_default_none(self):
        self.check_source(self.example().replace('local(work)','local(work) shared(particles) default(none)'))

    @staticmethod
    def reader(function=False):
        proc='''pure subroutine read_record(p,value)
type(t),intent(in) :: p
integer,intent(out) :: value
value=p%x+p%y(1)
end subroutine'''
        call='call read_record(w,a(i))'
        if function:
            proc='''pure integer function read_record(p) result(value)
type(t),intent(in) :: p
value=p%x+p%y(1)
end function'''
            call='a(i)=read_record(p=w)'
        return '''implicit none
type :: t
 integer :: x,y(2)
end type
type(t) :: w
integer :: i,a(9000)
do concurrent(i=1:9000) local(w)
 w%x=i
 w%y(1)=7
 CALL
end do
do i=1,9000
 if(a(i)/=i+7) stop 1
end do
print *, 'PASS'
contains
PROC
end
'''.replace('CALL',call).replace('PROC',proc)

    def test_in_does_not_read_unused_undefined_field(self):
        self.check_source(self.reader())

    def test_function(self):
        self.check_source(self.reader(True))

    def test_real_and_complex_function_results(self):
        for kind,conversion in [('real(8)','int(read_record(p=w))'),('complex(8)','int(real(read_record(p=w)))')]:
            with self.subTest(kind=kind):
                source=self.reader(True).replace('pure integer function',f'pure {kind} function').replace('a(i)=read_record(p=w)',f'a(i)={conversion}')
                self.check_source(source)

    def test_logical_function_result(self):
        source=self.reader(True).replace('pure integer function','pure logical function').replace('value=p%x+p%y(1)','value=p%x>0').replace(
            'a(i)=read_record(p=w)','a(i)=merge(w%x+7,-99,read_record(p=w))')
        self.check_source(source)

    def test_unused_undefined_scalar_field(self):
        for function in (False,True):
            with self.subTest(function=function):
                source=self.reader(function).replace('x,y(2)','x,unused').replace(' w%x=i',' w%x=i+7').replace(' w%y(1)=7\n','').replace('p%x+p%y(1)','p%x')
                self.check_source(source)

    def test_inout_does_not_read_unused_undefined_fields(self):
        self.check_source(self.reader().replace('x,y(2)','x,y(2),unused').replace('intent(in) :: p','intent(inout) :: p'))

    def test_borrowed_scalar_function_in_serial_mode(self):
        source=self.reader(True).replace('x,y(2)','x,unused').replace(' w%x=i',' w%x=i+7').replace(' w%y(1)=7\n','').replace('p%x+p%y(1)','p%x')
        self.build_run(source,'PASS',parallel=False,threads=(1,))

    def test_unused_undefined_complex_scalar(self):
        source=self.reader(True).replace('integer :: x,y(2)','integer :: x\ncomplex(8) :: unused').replace(' w%x=i',' w%x=i+7').replace(' w%y(1)=7\n','').replace('p%x+p%y(1)','p%x')
        self.check_source(source)

    def test_guarded_scalar_read_stays_lazy(self):
        source=self.reader(True).replace('x,y(2)','x,unused').replace(' w%x=i',' w%x=-i').replace(' w%y(1)=7',' if(mod(i,2)==0) w%unused=7').replace(
            'value=p%x+p%y(1)','value=0\nif(p%x>0) value=p%unused').replace('a(i)/=i+7','a(i)/=0')
        self.check_source(source)

    def test_undefined_field_read(self):
        self.reject(self.reader().replace('value=p%x+p%y(1)','value=p%x+p%y(2)'),'uninitialized')

    def test_runtime_undefined_field_read(self):
        self.check_source(self.reader().replace(' w%y(1)=7',' w%y(1)=7\n if(i==1) w%y(2)=8').replace('value=p%x+p%y(1)','value=p%x+p%y(2)'),runtime_error='uninitialized')

    def test_writable_record_alias_is_rejected(self):
        source=self.example().replace('call advance(dt=1.,p=work)','call advance(dt=work%state%velocity(1),p=work)')
        self.reject(source,'may alias writable storage')

    def test_impure_is_rejected(self):
        self.reject(self.example().replace('pure subroutine','subroutine'),'whole-object or unsupported component')

    def test_unproved_helper_uses_serial_execution(self):
        source=self.example().replace('p%state%position=p%state%position+dt*p%state%velocity',
            'p%state%position=pack(p%state%position+dt*p%state%velocity,.true.)')
        self.check_source(source,reason='uses serial execution')

    @staticmethod
    def writer(default=True):
        return '''implicit none
type :: t
 integer :: x
 integer :: y(2)DEFAULT
end type
type(t) :: w
integer :: i,a(9000)
w%x=99
w%y=88
do concurrent(i=1:9000) local_init(w)
 call fill(w,i)
 a(i)=w%x+sum(w%y)
end do
if(w%x/=99.or.any(w%y/=88)) stop 1
do i=1,9000
 if(a(i)/=i+14) stop 2
end do
print *, 'PASS'
contains
pure subroutine fill(p,index)
type(t),intent(out) :: p
integer,intent(in) :: index
p%x=index
end subroutine
end
'''.replace('DEFAULT','=7' if default else '')

    def test_out_applies_defaults(self):
        self.check_source(self.writer())

    def test_out_clears_previous_definition(self):
        self.reject(self.writer(False),'uninitialized')

    def test_nested_calls(self):
        source=self.example().replace('p%state%position=p%state%position+dt*p%state%velocity','call drift(p,dt)')
        source=source.rsplit('end',1)[0]+'''pure subroutine drift(p,dt)
type(particle),intent(inout) :: p
real,intent(in) :: dt
p%state%position=p%state%position+dt*p%state%velocity
end subroutine
end
'''
        self.check_source(source)

    def test_primitive_output_aliases_readonly_record(self):
        self.reject(self.reader().replace('call read_record(w,a(i))','call read_record(w,w%x)\na(i)=w%x'),
            'may alias writable storage')

    def test_two_writable_record_actuals_alias(self):
        source=self.example().replace('call advance(dt=1.,p=work)','call advance(work,work,1.)').replace(
            'subroutine advance(p,dt)','subroutine advance(p,q,dt)').replace('type(particle),intent(inout) :: p',
            'type(particle),intent(inout) :: p,q')
        self.reject(source,'may alias writable storage')

    def test_callee_host_record_is_not_shadowed(self):
        self.check_source('''module m
implicit none
type :: t
 integer :: x,y(2)
end type
type(t) :: work
contains
pure subroutine read_record(p,value)
type(t),intent(in) :: p
integer,intent(out) :: value
value=p%x+p%y(1)+work%x
end subroutine
end module
program main
use m
implicit none
integer :: i,a(9000)
work%x=99
do concurrent(i=1:9000) local(work)
 work%x=i
 work%y(1)=7
 call read_record(work,a(i))
end do
do i=1,9000
 if(a(i)/=i+106) stop 1
end do
if(work%x/=99) stop 2
print *, 'PASS'
end
''')

if __name__=='__main__':
    unittest.main()

"""Intrinsic whole-record copies involving DO CONCURRENT locality entities."""
import subprocess
import tempfile
import unittest
from pathlib import Path
from test_record_read import ROOT, COMPILER
import test_concurrent_parallel_local_records as local_records


class ParallelRecordAssignment(unittest.TestCase):
    check_source=local_records.ParallelLocalRecords.check_source
    build_run=local_records.ParallelLocalRecords.build_run
    reject=local_records.ParallelLocalRecords.reject

    @staticmethod
    def example():
        return (ROOT/'tests/programs/xconcurrent_parallel_record_assignment.f90').read_text()

    def test_example(self):
        self.check_source(self.example())

    def test_local_init(self):
        self.check_source(self.example().replace('local(work)','local_init(work)').replace('work%count=-99',
            'work=particles(1)\nwork%count=-99'))

    def test_serial(self):
        self.build_run(self.example(),'PASS',parallel=False,threads=(1,))

    def test_checks_off(self):
        self.build_run(self.example(),'PASS',checks=False)

    def test_default_none(self):
        self.check_source(self.example().replace('local(work)','local(work) shared(particles) default(none)'))

    def test_interpreter(self):
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'case.f90';path.write_text(self.example().replace('n=9000','n=3'))
            result=subprocess.run([str(COMPILER),'run',str(path)],cwd=tmp,capture_output=True,text=True,timeout=30)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.split(),['PASS'])

    @staticmethod
    def partial(read=False):
        return '''implicit none
type :: t
 integer :: x,y(2)
end type
type(t) :: a(9000),w
integer :: i,b(9000)
do i=1,9000
 a(i)%x=i
 a(i)%y(1)=7
end do
do concurrent(i=1:9000) local(w)
 w=a(i)
 a(i)=w
 b(i)=w%x+w%y(INDEX)
end do
do i=1,9000
 if(b(i)/=i+7) stop 1
end do
print *, 'PASS'
end
'''.replace('INDEX','2' if read else '1')

    def test_undefined_fields_are_transported(self):
        self.check_source(self.partial())

    def test_undefined_field_still_traps_when_read(self):
        self.check_source(self.partial(True),runtime_error='uninitialized')

    def test_copy_clears_previous_definition(self):
        source=self.partial(True).replace('do concurrent(i=1:9000) local(w)',
            'w%x=0\nw%y=99\ndo concurrent(i=1:9000) local_init(w)')
        self.check_source(source,runtime_error='uninitialized')

    def test_complex_undefined_fields(self):
        source=self.partial().replace('integer :: x,y(2)','integer :: x\n complex(8) :: y(2)').replace(
            'a(i)%y(1)=7','a(i)%y(1)=(7.0_8,3.0_8)').replace('w%x+w%y(1)','w%x+int(real(w%y(1)))')
        self.check_source(source)
        self.check_source(source.replace('int(real(w%y(1)))','int(real(w%y(2)))'),runtime_error='uninitialized')

    def test_cross_iteration_record_reads_fall_back(self):
        self.check_source(self.example().replace('work=particles(i)','work=particles(1)').replace(
            'work%state%position=work%state%position+work%state%velocity',
            'work%state%position=real(i)+work%state%velocity').replace('work%count=work%count+1','work%count=i+1').replace(
            'work%z=conjg(work%z)','work%z=cmplx(real(i),2.)'),reason='not confined to the current iteration')

    def test_different_types_are_rejected(self):
        source=self.partial().replace('type(t) :: a(9000),w','type :: other\n integer :: x,y(2)\nend type\ntype(t) :: a(9000)\ntype(other) :: w')
        self.reject(source,'same supported declared type')

    def test_two_private_records(self):
        self.check_source(self.partial().replace('a(9000),w','a(9000),w,v').replace('local(w)','local(w,v)').replace('a(i)=w','v=w\nw=v\na(i)=v'))

    def test_double_complex_and_integer_kinds(self):
        self.check_source(self.example().replace('complex ::','complex(8) ::').replace('integer :: count','integer(8) :: count'))

    def test_module_owners_retain_serial_fallback(self):
        source=self.example().replace('n=9000','n=30').replace('implicit none','module m\nimplicit none',1)
        source=source.replace('integer :: i\ndo i=1,n','contains\nsubroutine solve()\ninteger :: i\ndo i=1,n',1)
        source=source.rsplit('end',1)[0]+'end subroutine\nend module\nprogram p\nuse m\ncall solve()\nend\n'
        self.check_source(source,reason='writes to host or module storage')

    def test_use_associated_type(self):
        source=self.example()
        at=source.index('type(particle) :: particles')
        source='module m\n'+source[:at]+'end module\nprogram p\nuse m\nimplicit none\n'+source[at:]
        self.check_source(source)

    def test_source_bounds_are_checked(self):
        self.check_source(self.partial().replace('w=a(i)','w=a(i+9000)').replace(' a(i)=w\n',''),runtime_error='subscript outside')

    def test_destination_dependent_source_subscript_is_rejected(self):
        self.reject(self.partial().replace('w=a(i)','w%x=i\nw=a(w%x)'), 'destination-dependent source subscript')

    def test_defined_assignment_is_not_bypassed(self):
        source='''module m
type :: t
 integer :: x
end type
interface assignment(=)
 module procedure copy
end interface
contains
pure subroutine copy(a,b)
 type(t),intent(out) :: a
 type(t),intent(in) :: b
 a%x=b%x+1
end subroutine
end module
program p
use m
implicit none
type(t) :: w,a(9000)
integer :: i
do concurrent(i=1:9000) local(w)
 w=a(i)
 a(i)=w
end do
end
'''
        self.reject(source,'containing defined assignment')


if __name__=='__main__':
    unittest.main()

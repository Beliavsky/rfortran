"""Runtime multidimensional triplets across storage classes and element types."""
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_character_comparisons
from test_record_read import ROOT, COMPILER, build_commands


class RuntimeSections(unittest.TestCase):
    compare = test_character_comparisons.CharacterComparisons.compare

    def check(self, source, expected, timeout=15):
        with tempfile.TemporaryDirectory(prefix="runtime-sections-") as tmp:
            directory = Path(tmp)
            path = directory / "main.f90"
            path.write_text(source, encoding="utf-8")
            if timeout==15:
                self.compare(path, expected, directory)
            else:
                # Large CHARACTER selections exercise the interpreter's scalar
                # storage loops as well as the two native optimization levels.
                for command in build_commands(path,directory):
                    with self.subTest(command=command):
                        result=subprocess.run(command,capture_output=True,text=True,timeout=timeout)
                        self.assertEqual(result.returncode,0,result.stderr)
                        self.assertEqual(result.stderr,"")
                        self.assertTrue(test_character_comparisons.equivalent(result.stdout,expected),result.stdout)

    def test_original_reproducer(self):
        with tempfile.TemporaryDirectory(prefix="runtime-section-repro-") as tmp:
            self.compare(ROOT / "tests/programs/xruntime_matrix_section.f90", "1 2 3\n", Path(tmp))

    def test_local_overlap_lower_bounds_and_empty_sections(self):
        self.check("""implicit none
integer,allocatable::a(:,:)
integer::n,i,j
n=3
allocate(a(-1:1,2:n+2))
do j=2,n+2
  do i=-1,1
    a(i,j)=10*j+i
  end do
end do
print *,a(0,:),a(:,3)
a(:,3:4)=a(:,2:3)
print *,a(:,4),a(1:-1:-1,2)
print *,size(a(1:0,2:4)),size(a(1:-1:-1,2:5:2),1),size(a(1:-1:-1,2:5:2),2)
print *,sum(a(:,2))
end
""", "20 30 40 50 29 30 31\n29 30 31 21 20 19\n0 3 2\n60\n")

    def test_rank_three_real_logical_complex(self):
        self.check("""implicit none
real(8),allocatable::r(:,:,:)
logical,allocatable::l(:,:,:)
complex(8),allocatable::z(:,:,:)
integer::n
n=2
allocate(r(n,2,2),l(n,2,2),z(n,2,2))
r=2.5d0
l=.true.
z=cmplx(3.d0,4.d0,kind=8)
r(2,:,2)=7.d0
l(:,2,1)=.false.
z(2,:,2)=conjg(z(2,:,2))
print *,r(2,:,2),l(:,2,1),real(z(2,:,2)),aimag(z(2,:,2))
z(2,:,2)=z(2,:,2)*z(2,:,2)
print *,real(z(2,:,2)),aimag(z(2,:,2))
call show(r)
contains
subroutine show(a)
real(8),intent(in)::a(:,:,:)
print *,a(2,:,2)
end subroutine
end
""", "7 7 F F 3 3 -4 -4\n-7 -7 -24 -24\n7 7\n")

    def test_character_local_and_dummy(self):
        self.check("""implicit none
character(3),allocatable::a(:,:)
integer::n
n=3
allocate(a(2,n))
a='abc'
a(2,:)='def'
a(:,2:3)=a(:,1:2)
print '(6(a,1x))',a(1,:),a(2,3:1:-1)
print '(3(a,1x))',a(1,:)//'!'
print *,all(a(2,:)=='def'),all(a(2,:)==['def','def','def'])
call show(a)
contains
subroutine show(b)
character(*),intent(in)::b(:,:)
print '(2(a,1x))',b(:,2)
end subroutine
end
""", "abc abc abc def def def\nabc! abc! abc!\nT T\nabc def\n")

    def test_derived_local_and_dummy(self):
        self.check("""implicit none
type::entry
integer::number
real(8)::value
character(3)::label
end type entry
type(entry),allocatable::a(:,:)
integer::n
n=3
allocate(a(2,n))
a=entry(1,2.5d0,'abc')
a(2,:)=entry(4,7.5d0,'def')
a(:,2:3)=a(:,1:2)
call show(a)
contains
subroutine show(b)
type(entry),intent(in)::b(:,:)
print *,b(2,:)%number,b(1,:)%value
print '(3(a,1x))',b(2,:)%label
end subroutine
end
""", "4 4 4 2.5 2.5 2.5\ndef def def\n")

    def test_rank_fifteen(self):
        dims = ','.join([':']*15)
        extent = ','.join(['n']+['1']*14)
        selected = ','.join(['n:1:-1']+['1']*14)
        self.check(f"""implicit none
integer,allocatable::a({dims})
integer::n
n=2
allocate(a({extent}))
a=7
print *,a({selected})
end
""", "7 7\n")

    def test_writable_section_arguments_and_nested_forwarding(self):
        self.check("""implicit none
integer,allocatable::a(:,:)
integer::n
n=3
allocate(a(2,n))
call fill(a(1,:))
call fill(a(2,:))
call increment(a(:,2:3))
call forward(a)
print *,a
contains
subroutine fill(v)
integer,intent(out)::v(:)
v=10
end subroutine
subroutine increment(v)
integer,intent(inout)::v(:,:)
v=v+1
end subroutine
subroutine forward(v)
integer,intent(inout)::v(:,:)
call increment(v(:,1:2))
end subroutine
end
""", "11 11 12 12 11 11\n")

    def test_runtime_stride_and_single_evaluation(self):
        self.check("""implicit none
integer,allocatable::a(:,:)
integer::n,step,picked(2)
n=4
allocate(a(2,n))
a(1,:)=[1,2,3,4]
a(2,:)=[5,6,7,8]
step=-2
picked=a(row(),4:1:step)
print *,picked
print *,size(a(:,4:1:step),2)
step=2
a(:,1:4:step)=0
print *,a
contains
integer function row()
print *,"row"
row=2
end function row
end
""", "row\n8 6\n2\n0 0 2 6 0 0 4 8\n")

    def test_pointer_sections(self):
        self.check("""implicit none
integer,target::a(2,3)
integer,pointer::p(:,:)
a=reshape([1,2,3,4,5,6],[2,3])
p=>a
print *,p(1,:)
p(:,2:3)=p(:,1:2)
print *,a
end
""", "1 3 5\n1 2 1 2 3 4\n")

    def test_automatic_arrays(self):
        self.check("""implicit none
integer,allocatable::a(:,:)
integer::n
n=3
allocate(a(2,n))
a=5
call work(a)
contains
subroutine work(v)
integer,intent(in)::v(:,:)
integer::scratch(size(v,1),size(v,2))
character(2)::words(size(v,1),size(v,2))
scratch=v+1
words='ab'
print *,scratch(1,:)
print '(3(a,1x))',words(1,:)
end subroutine
end
""", "6 6 6\nab ab ab\n")

    def test_selected_uninitialized_and_out_of_bounds(self):
        self.failure("""implicit none
integer,allocatable::a(:,:)
integer::n
n=3
allocate(a(2,n))
a(1,:)=1
print *,a(2,:)
end
""", 'uninitialized variable')
        self.failure("""implicit none
integer,allocatable::a(:,:)
integer::n
n=3
allocate(a(2,n))
a=1
print *,a(1,1:n+1)
end
""", 'outside allocated bounds')
        self.failure("""implicit none
character(3),allocatable::a(:,:)
integer::n
n=2
allocate(a(n,n))
a(:,1)='abc'
print *,a(:,2)
end
""", 'element 3')

    def test_character_reallocation_empty_and_long_elements(self):
        self.check("""implicit none
character(1100),allocatable::a(:,:)
integer::n
n=2
allocate(a(2,n))
a='long'
print *,len(a),len_trim(a(2,1))
deallocate(a)
allocate(a(0,n))
print *,size(a(:,1:2))
deallocate(a)
allocate(a(1,n))
a='again'
print '(2(a,1x))',trim(a(1,1)),trim(a(1,2))
end
""", "1100 4\n0\nagain again\n")

    def test_character_sections_can_exceed_fixed_storage_capacity(self):
        self.check("""implicit none
character(1),allocatable::a(:,:)
integer::n
n=1025
allocate(a(1,n))
a='x'
print *,size(a(1:1,1:1025)),all(a(1,1:1025)=='x')
end
""", "1025 T\n", timeout=60)

    def test_module_array_sections(self):
        self.check("""module data
implicit none
character(3),allocatable::a(:,:)
contains
subroutine show()
print '(3(a,1x))',a(1,:)
end subroutine
end module
program main
use data
implicit none
integer::n
n=3
allocate(a(2,n))
a='abc'
call show()
end
""", "abc abc abc\n")

    def test_character_and_derived_writable_sections(self):
        self.check("""implicit none
type::entry
integer::number
character(2)::label
end type entry
type(entry),allocatable::a(:,:)
character(3),allocatable::words(:,:)
integer::n
n=2
allocate(a(2,n),words(2,n))
call fill(a(1,:))
call fill(a(2,:))
call fill_words(words(1,:))
call fill_words(words(2,:))
print *,a(2,:)%number
print '(2(a,1x))',a(1,:)%label
print '(2(a,1x))',words(2,:)
contains
subroutine fill(v)
type(entry),intent(out)::v(:)
v=entry(5,'hi')
end subroutine
subroutine fill_words(v)
character(*),intent(out)::v(:)
v='abc'
end subroutine
end
""", "5 5\nhi hi\nabc abc\n")

    def test_random_number_section_does_not_read_unset_elements(self):
        self.check("""implicit none
real,allocatable::a(:,:,:)
integer::n
n=2
allocate(a(n,n,n))
call random_number(a(1,:,:))
print *,all(a(1,:,:)>=0.),all(a(1,:,:)<1.)
end
""", "T T\n")

    def test_array_function_and_intrinsic_rhs(self):
        self.check("""implicit none
real,allocatable::a(:,:)
integer::n
n=3
allocate(a(2,n))
a(1,:)=make(n)
a(2,:)=sin(0.0)
print *,a(1,:),a(2,:)
contains
function make(n) result(v)
integer,intent(in)::n
real::v(n)
v=8.
end function
end
""", "8 8 8 0 0 0\n")

    def test_integer_kinds(self):
        for kind,value in [(1,'7'),(2,'1000'),(4,'1000000'),(8,'10000000001_8')]:
            with self.subTest(kind=kind):
                number=value.split('_')[0]
                self.check(f"""implicit none
integer({kind}),allocatable::a(:,:)
integer::n
n=2
allocate(a(2,n))
a={value}
a(:,2)=a(:,1)
print *,a(2,:)
end
""",f"{number} {number}\n")

    def test_readonly_sections_remain_rejected(self):
        source="""implicit none
integer,allocatable::a(:,:)
integer::n
n=2
allocate(a(n,n))
a=1
call work(a)
contains
subroutine work(v)
integer,intent(in)::v(:,:)
call fill(v(1,:))
end subroutine
subroutine fill(v)
integer,intent(out)::v(:)
v=2
end subroutine
end
"""
        with tempfile.TemporaryDirectory(prefix='runtime-section-readonly-') as tmp:
            path=Path(tmp)/'main.f90'
            path.write_text(source,encoding='utf-8')
            result=subprocess.run([COMPILER,'check',path],capture_output=True,timeout=30)
            self.assertNotEqual(result.returncode,0)
            self.assertIn(b'INTENT(IN)',result.stderr)

    def test_runtime_zero_stride_is_diagnosed(self):
        self.failure("""implicit none
integer,allocatable::a(:,:)
integer::n,step
n=2
allocate(a(n,n))
a=1
step=0
print *,a(:,1:n:step)
end
""", "stride must not be zero")

    def failure(self, source, error):
        with tempfile.TemporaryDirectory(prefix="runtime-section-error-") as tmp:
            directory=Path(tmp)
            path=directory/'main.f90'
            path.write_text(source,encoding='utf-8')
            for command in build_commands(path,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=20)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(error,result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)

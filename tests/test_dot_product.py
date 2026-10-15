"""DOT_PRODUCT types, ranks, evaluation and runtime conformance checks."""
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_character_comparisons
from test_record_read import ROOT, COMPILER, build_commands

class DotProduct(unittest.TestCase):
    compare = test_character_comparisons.CharacterComparisons.compare

    def check(self, source, expected):
        with tempfile.TemporaryDirectory(prefix="dot-product-") as tmp:
            directory=Path(tmp)
            path=directory/"main.f90"
            path.write_text(source,encoding="utf-8")
            self.compare(path,expected,directory)

    def test_xreal(self):
        self.check("""use iso_fortran_env,only:real64
implicit none
real(real64)::values(6)=[3.,4.,3.,-4.,25.,0.]
print *, 'values = ',values
print *, 'dot_product(values,values) = ',dot_product(values,values)
end
""","values = 3 4 3 -4 25 0\ndot_product(values,values) = 675\n")

    def test_numeric_kinds(self):
        self.check("""use iso_fortran_env,only:real64,int64
implicit none
real::a(3)
real(real64)::b(3)
integer::i(3)
integer(int64)::j(3)
a=[1.,2.,3.]
b=[2._real64,3._real64,4._real64]
i=[1,2,3]
j=[10000000001_int64,2_int64,3_int64]
print *,dot_product(a,a),dot_product(a,b),dot_product(b,a),kind(dot_product(a,b))
print *,dot_product(i,i),dot_product(i,j),dot_product(j,i),kind(dot_product(i,j))
print *,dot_product(i,b),dot_product(b,i),kind(dot_product(i,b))
end
""","14 20 20 8\n14 10000000014 10000000014 8\n20 20 8\n")

    def test_empty_and_logical(self):
        self.check("""implicit none
real::r(0)
integer(8)::i(0)
logical::empty(0),a(3),b(3)
a=[.true.,.false.,.true.]
b=[.false.,.true.,.false.]
print *,dot_product(r,r),dot_product(i,i),dot_product(empty,empty)
print *,dot_product(a,b),dot_product(.not.a,b),dot_product(a,a)
end
""","0 0 F\nF T T\n")

    def test_sections_vectors_and_keywords(self):
        self.check("""implicit none
integer::a(-1:2),b(4),m(2,3)
a=[1,2,3,4]
b=[10,20,30,40]
m=reshape([1,2,3,4,5,6],[2,3])
print *,dot_product(vector_b=b,vector_a=a)
print *,dot_product(a(2:-1:-1),b)
print *,dot_product(a([2,-1]),[4,5])
print *,dot_product(m(:,2),[2,3])
end
""","300\n200\n21\n18\n")

    def test_function_arguments_once_and_pure(self):
        self.check("""implicit none
integer::calls
calls=0
print *,dot_product(make(calls),make(calls))
print *,calls,norm([1.,2.,3.])
contains
function make(counter) result(v)
integer,intent(inout)::counter
real::v(3)
counter=counter+1
v=[1.,2.,3.]
end function
pure real function norm(a)
real,intent(in)::a(:)
norm=dot_product(a,a)
end function
end
""","14\n2 14\n")

    def test_complex_conjugation_and_promotion(self):
        self.check("""implicit none
complex(8)::a(2),b(2)
real::r(2)
a=[(1.d0,2.d0),(3.d0,4.d0)]
b=[(5.d0,6.d0),(7.d0,8.d0)]
r=[2.,3.]
print *,dot_product(vector_b=b,vector_a=a),kind(dot_product(a,b))
print *,dot_product(a,r),dot_product(r,a)
end
""","(70,-8) 8\n(11,-16) (11,16)\n")

    def test_user_procedure_shadows_intrinsic(self):
        self.check("""implicit none
print *,dot_product(b=3.,a=2.)
contains
real function dot_product(a,b)
real,intent(in)::a,b
dot_product=a-b
end function
end
""","-1\n")

    def test_kind_does_not_read_values(self):
        self.check("""implicit none
integer(8)::i(2)
real(8)::r(2)
logical::l(2)
print *,kind(dot_product(i,i)),kind(dot_product(i,r)),kind(dot_product(l,l))
print *,precision(dot_product(r,r)),range(dot_product(i,i))
end
""","8 8 4\n15 18\n")

    def test_loop_condition_is_reevaluated(self):
        self.check("""implicit none
integer::a(2),b(2),i
a=[1,2]
b=[1,1]
i=0
do while(dot_product(a,b)<7)
  i=i+1
  a=a+1
  cycle
end do
print *,i,a
end
""","2 3 4\n")

    def test_runtime_pack_shape_check(self):
        with tempfile.TemporaryDirectory(prefix="dot-pack-") as tmp:
            directory=Path(tmp)
            path=directory/"main.f90"
            path.write_text("""implicit none
integer::n
real::a(3)
a=[1.,2.,3.]
read(*,*)n
print *,dot_product(pack(a,a<=n),[1.,1.])
end
""",encoding="utf-8")
            for command in build_commands(path,directory):
                with self.subTest(command=command):
                    good=subprocess.run(command,input="2\n",capture_output=True,text=True,encoding="utf-8",timeout=15)
                    self.assertEqual(good.returncode,0,good.stderr)
                    self.assertEqual(float(good.stdout),3.)
                    bad=subprocess.run(command,input="3\n",capture_output=True,text=True,encoding="utf-8",timeout=15)
                    self.assertNotEqual(bad.returncode,0)
                    self.assertIn("conform",bad.stderr)

    def test_elemental_operand(self):
        self.check("""implicit none
real::a(3)
a=[1.,2.,3.]
print *,dot_product(twice(a),a)
contains
pure elemental real function twice(x)
real,intent(in)::x
twice=2*x
end function
end
""","28\n")

    def test_array_named_dot_product(self):
        self.check("""implicit none
real::dot_product(2)
dot_product=[2.,3.]
print *,dot_product(1)
end
""","2\n")

    def test_generic_dispatch_on_promoted_result(self):
        self.check("""module selectors
implicit none
interface which
module procedure single_value,double_value,integer_value,logical_value
end interface
contains
integer function single_value(x)
real,intent(in)::x
single_value=4
end function
integer function double_value(x)
real(8),intent(in)::x
double_value=8
end function
integer function integer_value(x)
integer(8),intent(in)::x
integer_value=88
end function
integer function logical_value(x)
logical,intent(in)::x
logical_value=1
end function
end module
program main
use selectors
implicit none
real::a(2)
real(8)::b(2)
integer(8)::i(2)
logical::m(2)
a=1.
b=2.d0
i=3_8
m=.true.
print *,which(dot_product(a,b)),which(dot_product(b,a))
print *,which(dot_product(i,i)),which(dot_product(m,m))
end
""","8 8\n88 1\n")

    def test_invalid(self):
        for declaration,expr,message in [
            ("real::a(2,2)","dot_product(a,a)","rank-one"),
            ("integer(16)::a(2)","dot_product(a,a)","INTEGER kinds 1, 2, 4 and 8"),
            ("real::a(2)","dot_product(a,1.)","rank-one"),
            ("real::a(2),b(3)","dot_product(a,b)","do not conform"),
            ("real::a(2)\nlogical::b(2)","dot_product(a,b)","two numeric arrays or two LOGICAL"),
            ("character(2)::a(2)","dot_product(a,a)","two numeric arrays or two LOGICAL"),
        ]:
            with self.subTest(expr=expr,declaration=declaration),tempfile.TemporaryDirectory(prefix="dot-invalid-") as tmp:
                path=Path(tmp)/"main.f90"
                path.write_text("implicit none\n"+declaration+"\nprint *,"+expr+"\nend\n",encoding="utf-8")
                result=subprocess.run([COMPILER,"emit-obj",path,Path(tmp)/"main.obj"],capture_output=True,text=True,encoding="utf-8",timeout=60)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(message,result.stderr)
                self.assertIn("main.f90:",result.stderr)
                self.assertNotIn("panicked",result.stderr)

    def test_runtime_shape_check(self):
        source="""implicit none
integer::n,m
real,allocatable::a(:),b(:)
read(*,*)n
read(*,*)m
allocate(a(n),b(m))
a=1.
b=2.
print *,dot_product(a,b)
end
"""
        with tempfile.TemporaryDirectory(prefix="dot-runtime-") as tmp:
            directory=Path(tmp)
            path=directory/"main.f90"
            path.write_text(source,encoding="utf-8")
            for command in build_commands(path,directory):
                with self.subTest(command=command):
                    good=subprocess.run(command,input="3\n3\n",capture_output=True,text=True,encoding="utf-8",timeout=15)
                    self.assertEqual(good.returncode,0,good.stderr)
                    self.assertEqual(float(good.stdout),6.)
                    bad=subprocess.run(command,input="2\n3\n",capture_output=True,text=True,encoding="utf-8",timeout=15)
                    self.assertNotEqual(bad.returncode,0)
                    self.assertIn("conform",bad.stderr)
                    self.assertNotIn("panicked",bad.stderr)

if __name__=="__main__":
    unittest.main()

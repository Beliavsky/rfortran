"""F2023 REDUCE locality, shared by interpreter and native O0/O2."""
import subprocess
import tempfile
import unittest
from pathlib import Path
import test_fortran2023 as support
from test_record_read import ROOT, COMPILER


class ConcurrentReduce(unittest.TestCase):
    check = support.Fortran2023.check
    reject = support.Fortran2023.reject

    def test_example(self):
        self.check((ROOT / "tests/programs/xconcurrent_reduce.f90").read_text(), " 25 5 5 15\n")

    def test_numeric_and_bitwise(self):
        self.check('''implicit none
integer::i,s=10,p=2,lo=99,hi=-99,ba=15,bo=16,bx=32
do concurrent(i=1:4) reduce(+:s) reduce(*:p) reduce(min:lo) reduce(max:hi) &
 & reduce(iand:ba) reduce(ior:bo) reduce(ieor:bx)
s=i+s
p=p*i
lo=min(i,lo,10)
hi=max(hi,i)
ba=iand(ba,i)
bo=ior(i,bo)
bx=ieor(bx,i)
end do
print *,s,p,lo,hi,ba,bo,bx
end
''', " 20 48 1 4 0 23 36\n")

    def test_logical_operators(self):
        self.check('''implicit none
integer::i
logical::a=.true.,b=.false.,c=.true.,d=.false.
do concurrent(i=1:3) reduce(.and.:a) reduce(.or.:b) reduce(.eqv.:c) reduce(.neqv.:d)
a=a.and.(i<3)
b=(i==2).or.b
c=c.eqv.(i<3)
d=d.neqv.(i==2)
end do
print *,a,b,c,d
end
''', " F T F T\n")

    def test_mask_stride_and_cycle(self):
        self.check('''implicit none
integer::i,j,s=7
do concurrent(i=5:1:-2,j=1:3,j/=2) reduce(+:s)
if(i==3) cycle
s=s+i*j
end do
print *,s
end
''', " 31\n")

    def test_empty_and_false_mask_keep_initial_value(self):
        self.check('''implicit none
integer::i,s=17,p=5
do concurrent(i=1:0) reduce(+:s) reduce(*:p)
s=s+i
p=p*i
end do
do concurrent(i=1:3,.false.) reduce(+:s)
s=s+i
end do
print *,s,p
end
''', " 17 5\n")

    def test_kind_and_real(self):
        self.check('''use iso_fortran_env,only:int64,real64
implicit none
integer::i
integer(int64)::s=3000000000_int64
real(real64)::a=2.0_real64,b=-1.0_real64
do concurrent(i=1:3) reduce(+:s,a) reduce(max:b)
s=s+int(i,int64)
a=a+real(i,real64)/2
b=max(b,real(i,real64))
end do
print *,s,int(a),int(b)
end
''', " 3000000006 5 3\n")

    def test_indexed_array_and_section(self):
        self.check('''implicit none
integer::i,a(-1:1),b(2,2)
a=10
b=20
do concurrent(i=1:3) reduce(+:a,b)
a(i-2)=a(i-2)+i
b(:,1)=b(:,1)+i
end do
print *,a,b
end
''', " 11 12 13 26 26 20 20\n")

    def test_repeated_updates_and_ordinary_nested_loop(self):
        self.check('''implicit none
integer::i,j,s=1
do concurrent(i=1:3) reduce(+:s)
do j=1,3
if(j==2) exit
s=s+i
end do
s=2+s
end do
print *,s
end
''', " 13\n")

    def test_procedure_and_module(self):
        self.check('''module m
implicit none
contains
pure integer function total(n) result(s)
integer,intent(in)::n
integer::i,s
s=10
do concurrent(i=1:n) reduce(+:s)
s=s+i
end do
end function
end module
use m
implicit none
print *,total(3)
end
''', " 16\n")

    def test_nested_reductions(self):
        self.check('''implicit none
integer::i,j,s=1,t(2)
t=0
do concurrent(i=1:2) reduce(+:s)
do concurrent(j=1:2) reduce(+:t)
t(i)=t(i)+j
end do
s=s+i
end do
print *,s,t
end
''', " 4 3 3\n")

    def test_standard_gate(self):
        source='integer::i,s=0\ndo concurrent(i=1:3) reduce(+:s)\ns=s+i\nend do\nend\n'
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/"standard.f90"
            path.write_text(source)
            for standard,ok in (("f2018",False),("f2023",True)):
                p=subprocess.run([COMPILER,"-std="+standard,"check",path],capture_output=True,text=True)
                self.assertEqual(p.returncode==0,ok,p.stderr)
                if not ok:self.assertIn("DO CONCURRENT REDUCE",p.stderr)

    def test_named_multidimensional_cycle(self):
        self.check('''implicit none
integer::i,j,s=0
outer: do concurrent(i=1:3,j=1:3) reduce(+:s)
if(j==2) cycle outer
s=s+i*j
end do outer
print *,s
end
''', " 24\n")

    def test_complex_add_and_multiply(self):
        self.check('''implicit none
integer::i
complex::s=(1.0,2.0),p=(1.0,0.0)
do concurrent(i=1:2) reduce(+:s) reduce(*:p)
s=s+cmplx(i,1)
p=p*cmplx(0,1)
end do
print *,int(real(s)),int(aimag(s)),int(real(p)),int(aimag(p))
end
''', " 4 4 -1 0\n")

    def test_minmax_integer_extremes(self):
        self.check('''implicit none
integer::i,s,t
s=-huge(s)-1
t=huge(t)
do concurrent(i=1:1) reduce(max:s) reduce(min:t)
s=max(s,-huge(0)-1)
t=min(t,huge(0))
end do
print *,s,t
end
''', " -2147483648 2147483647\n")

    def test_invalid_updates(self):
        for statement in ("s=i", "s=s+s", "s=s*i", "print *,s", "call f(s)", "if(s>0) s=s+i"):
            with self.subTest(statement=statement):
                self.reject(f'integer::i,s=0\ndo concurrent(i=1:3) reduce(+:s)\n{statement}\nend do\nend\n', "REDUCE")

    def test_invalid_clauses(self):
        for clause,message in (("reduce(+:s,s)","duplicate"),("reduce(+:i)","concurrent index"),("reduce(-:s)","unsupported REDUCE"),("reduce(.and.:s)","not suitable"),("reduce(+:missing)","declared intrinsic")):
            with self.subTest(clause=clause):
                self.reject(f'integer::i,s=0\ndo concurrent(i=1:3) {clause}\nend do\nend\n',message)

    def test_invalid_attributes(self):
        for declaration,attr in (("integer,parameter::s=1","parameter"),("integer,intent(in)::s","intent(in)"),("integer,optional::s","optional")):
            self.reject(f'subroutine f(s)\n{declaration}\ninteger::i\ndo concurrent(i=1:3) reduce(+:s)\nend do\nend subroutine\n',attr)

    def test_unsupported_runtime_storage(self):
        self.reject('integer,allocatable::s(:)\ninteger::i\nallocate(s(2))\ndo concurrent(i=1:3) reduce(+:s)\ns=s+i\nend do\nend\n', "not yet supported")

    def test_exit_and_return(self):
        for statement in ("exit","return"):
            self.reject(f'integer::i,s=0\ndo concurrent(i=1:3) reduce(+:s)\n{statement}\nend do\nend\n', "cannot leave a DO CONCURRENT")


    def test_cycle_cannot_bypass_inner_reduction_combination(self):
        self.reject('''integer::i,j,s=0,t=0
outer: do concurrent(i=1:3) reduce(+:s)
do concurrent(j=1:2) reduce(+:t)
cycle outer
end do
end do outer
end
''', "CYCLE cannot leave a DO CONCURRENT")

    def test_assumed_shape_and_mismatched_designator(self):
        self.reject('''subroutine f(a)
integer,intent(inout)::a(:)
integer::i
do concurrent(i=1:2) reduce(+:a)
a=a+i
end do
end subroutine
''', "explicit array bounds")
        self.reject('''integer::i,a(3)
a=0
do concurrent(i=1:2) reduce(+:a)
a(i)=a(i+1)+1
end do
end
''', "invalid REDUCE update")

    def test_bitwise_kind_and_rank_three_minmax(self):
        self.check('''use iso_fortran_env,only:int64
implicit none
integer::i,a(2,1,2),b(2,1,2)
integer(int64)::bits=-1_int64,high=4294967296_int64
a=100
b=-100
do concurrent(i=1:2) reduce(min:a) reduce(max:b) reduce(iand:bits) reduce(ior:high)
a=min(a,i)
b=max(b,i)
bits=iand(bits,int(i,int64))
high=ior(high,int(i,int64))
end do
print *,a,b,bits,high
end
''', " 1 1 1 1 2 2 2 2 0 4294967299\n")

    def test_explicit_runtime_shape(self):
        self.reject('''implicit none
integer::a(3)
a=10
call f(a,3)
print *,a
contains
subroutine f(a,n)
integer,intent(in)::n
integer,intent(inout)::a(n)
integer::i
do concurrent(i=1:n) reduce(+:a)
a(i)=a(i)+i
end do
end subroutine
end
''', "runtime-sized reduction storage is not yet supported")

    def test_reduce_is_contextual_and_array_can_be_named_reduce(self):
        self.check('''implicit none
integer::i,reduce(2)
reduce=10
do concurrent(i=1:2) reduce(+:reduce)
reduce(i)=reduce(i)+i
end do
reduce(1)=reduce(1)+1
print *,reduce
end
''', " 12 12\n")

    def test_diagnostics_have_statement_line(self):
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/"bad.f90"
            path.write_text('integer::i,s=0\ndo concurrent(i=1:2) reduce(+:s)\ns=3\nend do\nend\n')
            p=subprocess.run([COMPILER,"check",path],capture_output=True,text=True)
            self.assertNotEqual(p.returncode,0)
            self.assertIn("invalid REDUCE update",p.stderr)
            self.assertIn("bad.f90:3:",p.stderr)


if __name__ == "__main__":
    unittest.main()

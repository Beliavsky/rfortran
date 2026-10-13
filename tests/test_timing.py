"""Timing intrinsics in the interpreter, native O0/O2, and gfortran when available.
Run after cargo build --bin rfort: python tests/test_timing.py
"""
import unittest
import test_deallocate
from test_record_read import ROOT

class Timing(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT/"tests/169_timing.f90").read_text(encoding="utf-8"),"T T\nT T T\nT T T\n")

    def test_optional_and_real_rate(self):
        self.check("""implicit none
integer :: count,rate,maximum
integer(8) :: count8,rate8,maximum8
real :: r
real(8) :: r8
call system_clock()
call system_clock(count_rate=rate)
call system_clock(count_max=maximum)
call system_clock(count)
print *,count>=0,count<=maximum,rate>0
call system_clock(count_max=maximum8)
call system_clock(count_rate=rate8)
call system_clock(count=count8)
print *,count8>=0_8,count8<=maximum8,rate8>0_8
call system_clock(count_rate=r)
call system_clock(count8,r8,maximum8)
print *,r>0.0,r8>0.0_8,count8<=maximum8
end
""","T T T\nT T T\nT T T\n")

    def test_array_elements_and_out_dummies(self):
        self.check("""program main
implicit none
real(8) :: cpu(2)
integer(8) :: clocks(3),c,r,m
real(8) :: t
call capture(t,c,r,m)
print *,t>=0.0_8,c>=0_8,c<=m,r>0_8
call cpu_time(time=cpu(1))
call system_clock(count_max=clocks(3),count=clocks(1),count_rate=clocks(2))
print *,cpu(1)>=t,clocks(1)>=0_8,clocks(1)<=clocks(3),clocks(2)>0_8
contains
subroutine capture(t,c,r,m)
real(8),intent(out)::t
integer(8),intent(out)::c,r,m
call cpu_time(t)
call system_clock(c,r,m)
end subroutine
end program
""","T T T T\nT T T T\n")

    def test_repeated_loop_calls_and_precision(self):
        self.check("""implicit none
integer :: i,count,rate,maximum
integer(8) :: before,after
real(8) :: first,last,work
call cpu_time(first)
call system_clock(before)
work=0.0_8
do i=1,100000
work=work+sqrt(dble(i))
end do
call cpu_time(last)
call system_clock(after)
print *,last>=first,work>0.0_8,after>=before
call system_clock(count,rate,maximum)
print *,rate>0,maximum>0,count>=0,count<=maximum
end
""","T T T\nT T T T\n")

    def test_multidimensional_outputs(self):
        self.check("""implicit none
integer :: rate(2),counts(-1:0,3:4),maxima(2,2,2)
real(8) :: cpu(2,2)
call system_clock(count=counts(0,4),count_rate=rate(1),count_max=maxima(2,1,2))
call cpu_time(cpu(2,1))
print *,counts(0,4)>=0,counts(0,4)<=maxima(2,1,2),rate(1)>0,cpu(2,1)>=0.0_8
if (.true.) call cpu_time(cpu(1,2))
print *,cpu(1,2)>=cpu(2,1)
end
""","T T T T\nT\n")

    def test_user_procedure(self):
        self.check("""program main
real :: x
call cpu_time(time=x)
print *,x
contains
subroutine cpu_time(time)
real,intent(out)::time
time=42.0
end subroutine
end program
""","42.0\n")

    def test_invalid_output(self):
        cases=[("integer::x\ncall cpu_time(x)","REAL variable"),
               ("real::x\ncall system_clock(count=x)","INTEGER kind 4 or 8"),
               ("integer(2)::x\ncall system_clock(x)","INTEGER kind 4 or 8"),
               ("integer::x(2)\ncall system_clock(x)","scalar output"),
               ("real::x(2)\ncall cpu_time(x)","scalar output"),
               ("call cpu_time(1.0)","writable scalar variable"),
               ("integer,parameter::x=3\ncall system_clock(x)","writable scalar variable"),
               ("integer::x\ninteger(8)::r\ncall system_clock(x,r)","same kind")]
        for body,message in cases:
            with self.subTest(body=body): self.reject("implicit none\n"+body+"\nend\n",message)

    def test_keywords(self):
        for call,message in [("cpu_time()","missing required argument"),
                             ("cpu_time(x,x)","too many arguments"),
                             ("cpu_time(clock=x)","unknown keyword"),
                             ("system_clock(count=x,count=x)","duplicate argument"),
                             ("system_clock(count=x,x)","positional argument after keyword")]:
            with self.subTest(call=call): self.reject("real::x\ncall "+call+"\nend\n",message)

    def test_readonly_and_pure(self):
        self.reject("""subroutine s(x)
real,intent(in)::x
call cpu_time(x)
end subroutine
""","writable variable")
        for intrinsic,kind in [("cpu_time","real"),("system_clock","integer")]:
            with self.subTest(intrinsic=intrinsic):
                self.reject("pure subroutine s(x)\n"+kind+",intent(out)::x\ncall "+intrinsic+"(x)\nend subroutine\n","PURE")

if __name__=="__main__": unittest.main()

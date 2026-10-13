"""DATE_AND_TIME properties in the interpreter, native O0/O2 and gfortran.
Run after cargo build --bin rfort: python tests/test_date_and_time.py
"""
import unittest
import test_deallocate
from test_record_read import ROOT

class DateAndTime(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_consistent_snapshot(self):
        self.check((ROOT/"tests/170_date_and_time.f90").read_text(encoding="utf-8"),"T T T T\nT T T T\nT T T\n")

    def test_optional_outputs_and_padding(self):
        self.check("""implicit none
character(12)::date,time
character(7)::zone
character(3)::short_date
integer::v(8)
call date_and_time()
call date_and_time(date=date)
call date_and_time(time=time)
call date_and_time(zone=zone)
call date_and_time(values=v)
call date_and_time(short_date)
print *,date(9:12)=='    ',time(11:12)=='  ',zone(6:7)=='  '
print *,len_trim(short_date)==3,v(1)>=2000,v(2)>=1 .and. v(2)<=12
end
""","T T T\nT T T\n")

    def test_integer_kinds_and_lower_bound(self):
        self.check("""use, intrinsic :: iso_fortran_env, only:int16,int64
implicit none
integer(int16)::small(8)
integer(int64)::large(-2:7)
large(6)=42
large(7)=43
call date_and_time(values=small)
call date_and_time(values=large)
print *,small(1)>=2000,large(-2)>=2000,large(5)>=0 .and. large(5)<=999
print *,large(6),large(7)
end
""","T T T\n42 43\n")

    def test_character_array_elements(self):
        self.check("""implicit none
character(8)::date(2)
character(10)::time(2)
character(5)::zone(2)
integer::v(8)
call date_and_time(date(2),time(2),zone(1),v)
print *,len_trim(date(2))==8,time(2)(7:7)=='.',len_trim(zone(1))==5,v(1)>=2000
end
""","T T T T\n")

    def test_output_dummies(self):
        self.check("""program main
implicit none
character(8)::date
integer::v(8)
call capture(date,v)
print *,len_trim(date)==8,v(1)>=2000,v(8)>=0 .and. v(8)<=999
contains
subroutine capture(date,v)
character(*),intent(out)::date
integer,intent(out)::v(:)
call date_and_time(values=v,date=date)
end subroutine
end program
""","T T T\n")

    def test_invalid_arguments(self):
        cases=[("integer::x\ncall date_and_time(date=x)","scalar CHARACTER"),
               ("character(8)::x(2)\ncall date_and_time(date=x)","scalar CHARACTER"),
               ("integer::v\ncall date_and_time(values=v)","rank-one INTEGER"),
               ("real::v(8)\ncall date_and_time(values=v)","rank-one INTEGER"),
               ("integer(1)::v(8)\ncall date_and_time(values=v)","rank-one INTEGER"),
               ("integer::v(2,4)\ncall date_and_time(values=v)","rank-one INTEGER"),
               ("call date_and_time(date='constant')","writable variable"),
               ("character(8)::d\ncall date_and_time(bad=d)","unknown keyword"),
               ("character(8)::d\ncall date_and_time(date=d,date=d)","duplicate argument")]
        for source,message in cases:
            with self.subTest(source=source): self.reject("implicit none\n"+source+"\nend\n",message)

    def test_short_values(self):
        self.check("integer::v(7)\ncall date_and_time(values=v)\nend\n",error="requires at least 8 elements")

    def test_readonly_and_pure(self):
        self.reject("subroutine s(d)\ncharacter(8),intent(in)::d\ncall date_and_time(date=d)\nend subroutine\n","writable variable")
        self.reject("pure subroutine s(v)\ninteger,intent(out)::v(8)\ncall date_and_time(values=v)\nend subroutine\n","PURE")

if __name__=="__main__": unittest.main()

"""Lazy Fortran 2023 conditional expressions and actual arguments."""
import unittest
import test_fortran2023 as support

class Conditionals(unittest.TestCase):
    check=support.Fortran2023.check
    reject=support.Fortran2023.reject

    def test_scalar_choices_and_precedence(self):
        self.check("""implicit none
integer::i
real::x=2.0
do i=-1,1
print *, (i<0 ? -1 : i==0 ? 0 : 1), 2*(i<0 ? 3 : 4)
end do
print *, (x>0 ? x : 0.0)
print *, (.true. ? (.false. ? 7 : 8) : 9)
end
""", " -1 6\n 0 8\n 1 8\n 2.0\n 8\n")

    def test_lazy_conditions_and_consequents(self):
        self.check("""implicit none
integer::calls=0,i=0
print *, (test(.true.) ? value(10) : test(.true.) ? value(20) : value(30))
print *,calls
print *, (i==0 ? 42 : 1/i)
print *, (.false. ? value(1) : test(.false.) ? value(2) : value(3))
print *,calls
contains
logical function test(b) result(r)
logical,intent(in)::b
calls=calls+1
r=b
end function
integer function value(i) result(r)
integer,intent(in)::i
calls=calls+10
r=i
end function
end
""", " 10\n 11\n 42\n 3\n 22\n")

    def test_conditional_variable_and_absent_argument(self):
        self.check("""implicit none
integer::x=1,y=2,i
call change((x>0 ? x : y), (x<0 ? 10 : .nil.))
print *,x,y
call change((.false. ? x : y), b=(.true. ? 5 : .nil.))
print *,x,y
do i=0,1
print *,f((i==0 ? .nil. : x), 3)
end do
contains
subroutine change(a,b)
integer,intent(inout)::a
integer,intent(in),optional::b
a=a+10
if(present(b)) a=a+b
end subroutine
integer function f(a,b) result(r)
integer,intent(in),optional::a
integer,intent(in)::b
r=b
if(present(a)) r=r+a
end function
end
""", " 11 2\n 11 17\n 3\n 14\n")

    def test_arrays_character_and_derived_values(self):
        self.check("""implicit none
type t
integer::i
end type
type(t)::a,b,c
integer::x(2)=[1,2],y(2)=[3,4],z(2)
character(8)::s
logical::yes=.true.
a%i=10
b%i=20
c=(yes ? a : b)
z=(yes ? x : y)
s=(yes ? "a" : "longer")
print *,c%i,z
print "(a,at,a)","[",s,"]"
call set((yes ? x : y))
print *,x,y
contains
subroutine set(a)
integer,intent(out)::a(2)
a=9
end subroutine
end
""", " 10 1 2\n[a]\n 9 9 3 4\n")

    def test_while_rechecks_after_cycle(self):
        self.check("""implicit none
integer::i=0
do while ((i<3 ? .true. : .false.))
i=i+1
if(i==2) cycle
print *,i
end do
print *,i
end
""", " 1\n 3\n 3\n")

    def test_constant_initializers(self):
        self.check("""implicit none
integer,parameter::n=(.true. ? 2+1 : 1/0)
logical,parameter::yes=(n==3 ? .true. : .false.)
character(*),parameter::s=(yes ? "ok" : "wrong")
real,parameter::r=(yes ? 1.0+2.0 : 0.0)
integer::a(n)
a=1
print *,n,yes,r,a
print "(a)",s
end
""", " 3 T 3.0 1 1 1\nok\n")

    def test_specification_expressions_and_pure_optional_guard(self):
        self.check("""implicit none
print *, get(),get(5)
call work(.true.)
call work(.false.)
contains
pure integer function get(x) result(r)
integer,intent(in),optional::x
r=(present(x) ? x : 0)
end function
subroutine work(short)
logical,intent(in)::short
integer::a((short ? 2 : 3))
a=7
print *,size(a),a
end subroutine
end
""", " 0 5\n 2 7 7\n 3 7 7 7\n")

    def test_invalid_conditions_and_types(self):
        for expr,message in [('(1 ? 2 : 3)',"scalar LOGICAL"),
            ('([.true.] ? 2 : 3)',"scalar LOGICAL"),
            ('(.true. ? 2 : 3.0)',"same declared type, kind and rank"),
            ('(.true. ? 2 : [3])',"same declared type, kind and rank"),
            ('(.true. ? 2 : 3_8)',"same declared type, kind and rank"),
            ('(.true. ? 1 : .nil.)',"conditional argument"),('.nil.',"only permitted")]:
            with self.subTest(expr=expr):
                self.reject(f'print *,{expr}\nend\n',message)

    def test_named_kinds_and_logical_intrinsics(self):
        self.check("""use iso_fortran_env,only:real64
implicit none
integer,parameter::dp=real64
real(dp)::x=1.0_dp
integer::a(2)=[1,2]
print *, (all(a>0) ? 1 : 0), (btest(1,0) ? 1 : 0)
print *, (x>0.0_dp ? x : 0.0_dp)
end
""", " 1 1\n 1.0\n")
        self.reject("""use iso_fortran_env,only:int64
implicit none
integer(int64),parameter::wide=1
integer,parameter::bad=(.true. ? wide : 0)
print *,bad
end
""", "same declared type, kind and rank")

    def test_incompatible_arguments(self):
        for argument,message in [('(.true. ? x : 10)',"writable"),
            ('(.true. ? x : (.false. ? x : y))',"writable"),
            ('(.true. ? x : (.false. ? y : .nil.))',"conditional argument")]:
            self.reject(f"""implicit none
integer::x=1,y=2
call sub({argument})
contains
subroutine sub(i)
integer,intent(out)::i
i=5
end subroutine
end
""",message)
        self.reject("""implicit none
integer,allocatable::a(:)
integer::b(2)
allocate(a(2))
call sub((.true. ? a : b))
contains
subroutine sub(i)
integer,intent(in)::i(:)
end subroutine
end
""", "agree in ALLOCATABLE and POINTER")

    def test_implied_do(self):
        self.check("""implicit none
integer::i,n=3,a(3)
print *, ((i==2 ? 20 : i),i=1,n)
print "(3i3)", ((i==2 ? 20 : i),i=1,n)
a=[( (i==2 ? 20 : i), i=1,n)]
print *,a
end
""", " 1 20 3\n  1 20  3\n 1 20 3\n")

    def test_function_choices_in_constructor_implied_do(self):
        self.check("""implicit none
integer::i,n=3,a(3),calls=0
a=[((i>1 ? twice(i) : i),i=1,n)]
print *,a,calls
contains
integer function twice(i) result(j)
integer,intent(in)::i
calls=calls+1
j=2*i
end function
end
""", " 1 4 6 2\n")

    def test_pointer_arguments_keep_selected_target(self):
        self.check("""implicit none
integer,target::x=1,y=2
integer,pointer::p,q
logical::yes=.true.
p=>x
q=>y
call inc((yes ? q : p))
print *,x,y
contains
subroutine inc(x)
integer,pointer,intent(inout)::x
x=x+10
end subroutine
end
""", " 1 12\n")

    def test_standard_selection_and_diagnostic_location(self):
        import subprocess,tempfile
        from pathlib import Path
        from test_record_read import COMPILER
        with tempfile.TemporaryDirectory() as tmp:
            source=Path(tmp)/"conditional.f90"
            source.write_text('implicit none\ninteger::x=1\nprint *, (x>0 ? x : 0)\nend\n')
            for std,success in [("f2018",False),("f2023",True)]:
                result=subprocess.run([COMPILER,"--std",std,"check",source],capture_output=True,text=True)
                self.assertEqual(result.returncode==0,success,result.stderr)
                if not success:
                    self.assertIn("2023",result.stderr)
                    self.assertIn("conditional.f90:3:",result.stderr)
            source.write_text('implicit none\ninteger::x=1\nprint *, (x>0 ? x : 0.0)\nend\n')
            result=subprocess.run([COMPILER,"check",source],capture_output=True,text=True)
            self.assertNotEqual(result.returncode,0)
            self.assertIn("conditional.f90:3:",result.stderr)

    def test_module_generic_and_complex(self):
        self.check("""module m
implicit none
interface pick
module procedure pick_int,pick_real
end interface
contains
integer function pick_int(i) result(r)
integer,intent(in)::i
r=i+10
end function
real function pick_real(i) result(r)
real,intent(in)::i
r=i+20.0
end function
subroutine optional_sub(i)
integer,intent(in),optional::i
if(present(i)) print *,i
end subroutine
end module
use m
implicit none
logical::yes=.true.
complex::a=(1.0,2.0),b=(3.0,4.0),c
print *,pick((yes ? 1 : 2)),pick((yes ? 1.0 : 2.0))
c=(yes ? a : b)
print *,real(c),aimag(c)
call optional_sub((yes ? 7 : .nil.))
call optional_sub((.not.yes ? 7 : .nil.))
end
""", " 11 21.0\n 1.0 2.0\n 7\n")

    def test_nil_in_specification_and_read_target(self):
        self.reject('integer(kind=(.true. ? 4 : .nil.))::x\nx=1\nend\n',"conditional argument")
        self.reject('integer::x,y\nread *, (.true. ? x : y)\nend\n',"writable READ item")

    def test_nil_requires_optional_dummy(self):
        self.reject("""implicit none
call sub((.true. ? 1 : .nil.))
contains
subroutine sub(i)
integer,intent(in)::i
print *,i
end subroutine
end
""", "missing required argument")

if __name__=="__main__":
    unittest.main()

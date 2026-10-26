"""NAMELIST in interpreter/native O0/O2, with gfortran round trips."""
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from test_record_read import ROOT, COMPILER, build_commands
import test_deallocate

class Namelist(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_round_trip(self):
        self.check((ROOT / "xnamelist.f90").read_text(encoding="utf-8"),
                   "3 1.0 2.0 3.0 T\nhello 'world\n")

    def test_partial_updates_repetition_nulls_comments_case(self):
        self.check("""implicit none
integer :: a(-1:2,2), n, ios
logical :: flag
character(12) :: s
namelist /settings/ a,n,flag,s
a=-7
n=9
flag=.false.
s='unchanged'
read(*,nml=settings,iostat=ios)
print *,ios,a,n,flag
print '(a)',s
end
""", "0 4 4 -7 6 -7 -7 -7 -7 9 T\na!b/'c      \n",
            data="&unrelated n=123 /\n! comment\n&SeTTings A=2*4,,6 ! comment\n flag=.true., s='a!b/''c' / ignored\n")

    def test_element_designators(self):
        self.check("""integer::a(-1:2,0:1),ios
namelist /g/ a
a=-1
read(*,nml=g,iostat=ios)
print *,ios,a
end
""", "0 -1 10 11 -1 -1 -1 -1 20\n",data="&g a(0,0)=10,a(1,0)=11, a(2,1)=20 /\n")
        for designator in ("a(-2,0)","a(0)","a(0:1,0)"):
            self.check("integer::a(-1:2,0:1),ios\nnamelist /g/ a\na=7\nread(*,nml=g,iostat=ios)\nprint *,ios>0,all(a==7)\nend\n",
                       "T T\n",data="&g "+designator+"=5 /\n",reference=False)

    def test_purity_intents_and_control_types(self):
        self.reject("program p\ncontains\npure subroutine f(n)\ninteger,intent(out)::n\nnamelist /g/ n\nread(*,nml=g)\nend subroutine\nend program\n", "PURE procedure cannot perform external I/O")
        self.reject("program p\ncontains\nsubroutine f(n)\ninteger,intent(in)::n\nnamelist /g/ n\nread(*,nml=g)\nend subroutine\nend program\n", "INTENT(IN)")
        self.reject("integer::n\nnamelist /g/ n\nread(*,nml=g,iostat=0)\nend\n", "require scalar variables")

    def test_internal_input_and_many_group_declarations(self):
        self.check("""implicit none
integer :: a,b,c,ios
character(100) :: record,msg
namelist /one/ a /two/ b,c
namelist /one/ b
a=1
b=2
c=3
msg='unchanged'
record='&ONE B=8, A=7 /'
read(unit=record,iomsg=msg,nml=one,iostat=ios)
print *,ios,a,b,c
print '(a)',trim(msg)
end
""", "0 7 8 3\nunchanged\n")

    def test_complex_kinds_and_character_array(self):
        self.check("""implicit none
integer(kind=1)::i1
integer(kind=2)::i2
integer(kind=8)::i8
real(kind=8)::r
complex(kind=8)::z(2)
character(6)::s(2)
integer::u,ios
namelist /values/ i1,i2,i8,r,z,s
i1=-12
i2=32000
i8=1234567890123_8
r=0.125_8
z=[(1.0_8,2.0_8),(3.0_8,-4.0_8)]
s=['a b   ',"c'd   "]
open(newunit=u,status='scratch')
write(u,nml=values)
rewind(u)
i1=0
i2=0
i8=0
r=0
z=(0.0_8,0.0_8)
s=''
read(u,nml=values,iostat=ios)
print *,ios,i1,i2,i8,r
print *,real(z(1)),aimag(z(1)),real(z(2)),aimag(z(2))
print '(a)',s
close(u)
end
""", "0 -12 32000 1234567890123 0.125\n1.0 2.0 3.0 -4.0\na b   \nc'd   \n")

    def test_status_errors(self):
        program="""implicit none
integer::n,ios
character(100)::msg
namelist /g/ n
n=99
msg=''
read(*,nml=g,iostat=ios,iomsg=msg)
print *,ios/=0,len_trim(msg)>0,n
end
"""
        for data in ("&g unknown=1 /\n", "&g n='bad' /\n", "&g n=1,2 /\n", "&g n(1)=2 /\n", "&g n=" , ""):
            # Invalid input may leave a successfully read preceding value set.
            expected="T T 1\n" if data=="&g n=1,2 /\n" else "T T 99\n"
            self.check(program,expected,data=data,reference=False)

    def test_error_without_iostat(self):
        self.check("integer::n\nnamelist /g/ n\nread(*,nml=g)\nend\n",
                   error="NAMELIST input/output error",data="&g unknown=1 /\n")

    def test_omitted_variable_is_not_initialized(self):
        self.check("integer::n,m\nnamelist /g/ n,m\nread(*,nml=g)\nprint *,m\nend\n",
                   error="uninitialized variable `m`",data="&g n=1 /\n")

    def test_partial_array_initialization(self):
        self.check("integer::a(3)\nnamelist /g/ a\nread(*,nml=g)\nprint *,a(1)\nprint *,a(2)\nend\n",
                   error="element 2",data="&g a=4 /\n")
        self.check("integer::a(3)\nnamelist /g/ a\nread(*,nml=g)\nprint *,a\nend\n",
                   "4 5 6\n",data="&g a=4,5,6 /\n")

    def test_host_association(self):
        self.check("""program p
integer::n
namelist /g/ n
call read_g()
print *,n
contains
subroutine read_g()
read(*,nml=g)
end subroutine
end program
""", "42\n",data="&g n=42 /\n")

    def test_positional_group(self):
        self.check("integer::n,u\nnamelist /g/ n\nn=42\nopen(newunit=u,status='scratch')\nwrite(u,g)\nrewind(u)\nn=0\nread(u,g)\nprint *,n\nclose(u)\nend\n", "42\n")

    def test_bad_controls_and_unsupported_storage(self):
        for statement,message in (("read(*,nml=g,fmt=*)", "NAMELIST currently supports"),
                                  ("read(*,nml=g) n", "cannot have an input/output item list"),
                                  ("read(*,nml=absent)", "unknown NAMELIST group")):
            self.reject("integer::n\nnamelist /g/ n\n"+statement+"\nend\n",message)
        self.reject("integer,allocatable::n(:)\nnamelist /g/ n\nread(*,nml=g)\nend\n", "nonallocatable, nonpointer")

    def test_rank_fifteen(self):
        dims=','.join(['2']+['1']*14)
        subs=','.join(['2']+['1']*14)
        self.check(f"integer::a({dims}),ios\nnamelist /g/ a\na=0\nread(*,nml=g,iostat=ios)\nprint *,ios,sum(a)\nend\n", "0 9\n",data=f"&g a({subs})=9 /\n",std="f2018")

    def test_disconnected_units_and_multiple_reads(self):
        self.check("""integer::n,ios,u
character(100)::msg
namelist /g/ n
n=5
write(999,nml=g,iostat=ios,iomsg=msg)
print *,ios>0,len_trim(msg)>0
open(newunit=u,status='scratch')
write(u,'(a)')'&g n=7 /'
write(u,'(a)')'&g n=8 /'
rewind(u)
read(u,nml=g,iostat=ios)
print *,ios,n
read(u,nml=g,iostat=ios)
print *,ios,n
close(u)
end
""", "T T\n0 7\n0 8\n",reference=False)

    def test_standard_output(self):
        self.check("integer::n\nnamelist /g/ n\nn=7\nwrite(*,nml=g)\nend\n", "&g\n n=\n7,\n/\n",reference=False)

    def test_standard_gates(self):
        with tempfile.TemporaryDirectory(prefix="nml-standard-") as tmp:
            source=Path(tmp)/"example.f90"
            for program,message in (("integer::n,ios\ncharacter(100)::msg\nnamelist /g/ n\nread(*,nml=g,iostat=ios,iomsg=msg)\nend\n","IOMSG"),
                                    ("integer::n\ncharacter(100)::s\nnamelist /g/ n\ns='&g n=1 /'\nread(s,nml=g)\nend\n","internal NAMELIST input")):
                source.write_text(program,encoding="utf-8")
                result=subprocess.run([COMPILER,"-std=f90","run",source],capture_output=True,text=True,timeout=30)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(message,result.stderr)
                self.assertIn("2003",result.stderr)

    def test_token_limit_reports_error(self):
        self.check("integer::n,ios\nnamelist /g/ n\nn=8\nread(*,nml=g,iostat=ios)\nprint *,ios>0,n\nend\n",
                   "T 8\n",data="&g n="+"1"*5000+" /\n",reference=False)

    @unittest.skipUnless(shutil.which("gfortran"),"gfortran is unavailable")
    def test_cross_compiler_files(self):
        with tempfile.TemporaryDirectory(prefix="namelist-cross-") as tmp:
            d=Path(tmp)
            source=d/"example.f90"
            source.write_text("""implicit none
integer::n,u,mode
real(kind=8)::x(3)
character(12)::s
namelist /settings/ n,x,s
read(*,*)mode
if (mode==1) then
n=41
x=[0.125_8,-2.5_8,1.0e100_8]
s="a b/'c"
open(newunit=u,file='values.nml',status='replace')
write(u,nml=settings)
else
open(newunit=u,file='values.nml',status='old')
read(u,nml=settings)
print *,n,all(x==[0.125_8,-2.5_8,1.0e100_8])
print '(a)',trim(s)
end if
close(u)
end
""",encoding="utf-8")
            commands=build_commands(source,d,cwd=d)
            ref=d/"reference.exe"
            r=subprocess.run(["gfortran",source,"-o",ref],capture_output=True,text=True,timeout=60,cwd=d)
            self.assertEqual(r.returncode,0,r.stderr)
            for command in commands:
                for writer,reader in ((command,[ref]),([ref],command)):
                    r=subprocess.run(writer,input="1\n",capture_output=True,text=True,timeout=30,cwd=d)
                    self.assertEqual(r.returncode,0,r.stderr)
                    r=subprocess.run(reader,input="2\n",capture_output=True,text=True,timeout=30,cwd=d)
                    self.assertEqual(r.returncode,0,r.stderr)
                    self.assertEqual(r.stdout.split(),["41","T","a","b/'c"])

if __name__=="__main__":
    unittest.main(verbosity=2)

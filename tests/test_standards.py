"""Partial standard checking before lowering, including separately built modules."""
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER


class Standards(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix="rfort-standards-")
        self.addCleanup(self.temp.cleanup)
        self.work=Path(self.temp.name)

    def invoke(self, source, standard=None, command="parse", extra=()):
        path=self.work / "case.f90"
        path.write_text(source,encoding="utf-8")
        args=[str(COMPILER)]
        if standard: args.append("-std="+standard)
        args += list(extra)
        if command: args.append(command)
        args.append(str(path))
        if command=="parse": args.append(str(self.work / "ast.json"))
        return subprocess.run(args,cwd=self.work,capture_output=True,text=True,timeout=60)

    def assert_feature(self, source, before, after, feature):
        rejected=self.invoke(source,before)
        self.assertNotEqual(rejected.returncode,0,rejected.stdout)
        self.assertIn(feature,rejected.stderr)
        self.assertIn("selected standard is Fortran",rejected.stderr)
        self.assertIn("case.f90:",rejected.stderr)
        accepted=self.invoke(source,after)
        self.assertEqual(accepted.returncode,0,accepted.stderr)
        self.assertNotIn("requires Fortran",accepted.stderr)

    def test_f90_baseline_runs_in_every_mode(self):
        source="implicit none\ninteger :: a(2)\na=(/1,2/)\nprint *,sum(a)\nend\n"
        for standard in (None,"f90","f95","f2003","f2008","f2018","f2023"):
            with self.subTest(standard=standard):
                result=self.invoke(source,standard,command=None,extra=("--run","-o",str(self.work / "main.exe")))
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(result.stdout.split(),["3"])

    def test_f95_features(self):
        cases=[
            ("module m\ncontains\npure integer function f(x)\ninteger,intent(in)::x\nf=x\nend function\nend module\n", "PURE"),
            ("module m\ncontains\nelemental integer function f(x)\ninteger,intent(in)::x\nf=x\nend function\nend module\n", "ELEMENTAL"),
            ("integer :: a(2),i\nforall(i=1:2) a(i)=i\nend\n", "FORALL"),
            ("type t\ninteger :: x=1\nend type\nend\n", "component default initialization"),
            ("real :: t\ncall cpu_time(t)\nend\n", "cpu_time"),
        ]
        for source,feature in cases:
            with self.subTest(feature=feature): self.assert_feature(source,"f90","f95",feature)

    def test_procedure_diagnostic_points_only_to_its_header(self):
        source="module m\nimplicit none\ncontains\npure subroutine s()\nend subroutine\nend module\n"
        result=self.invoke(source,"f90")
        self.assertNotEqual(result.returncode,0)
        self.assertEqual(result.stderr.count("PURE procedures"),1,result.stderr)
        self.assertIn("case.f90:4:",result.stderr)

    def test_f2003_features_and_constructor_spelling(self):
        cases=[
            ("integer :: a(2)\na=[1,2]\nend\n", "square-bracket"),
            ("integer :: a(1)\na=(/integer :: 1/)\nend\n", "typed array constructors"),
            ('character(3)::s="abc"\nprint "(a)",[character(3)::s,"def"]\nend\n', "typed array constructors"),
            ("use,intrinsic :: iso_fortran_env,only:real64\nend\n", "intrinsic modules"),
            ("character(:),allocatable :: s\nend\n", "deferred CHARACTER"),
            ("subroutine s(a)\ninteger,allocatable,intent(inout)::a(:)\nend subroutine\n", "ALLOCATABLE dummy"),
            ("integer :: a\na=command_argument_count()\nend\n", "command_argument_count"),
            ("integer :: u\nflush(u)\nend\n", "FLUSH"),
            ("integer :: u,ios\ncharacter(80)::msg\nopen(unit=u,iostat=ios,iomsg=msg)\nend\n", "IOMSG"),
        ]
        for source,feature in cases:
            with self.subTest(feature=feature): self.assert_feature(source,"f95","f2003",feature)

    def test_f2008_features(self):
        cases=[
            ("error stop 1\nend\n", "ERROR STOP"),
            ("block\ninteger :: i\ni=1\nend block\nend\n", "BLOCK"),
            ("integer :: u\nopen(newunit=u,status='scratch')\nclose(u)\nend\n", "NEWUNIT"),
            ("real :: x(1,1,1,1,1,1,1,1)\nend\n", "rank greater than seven"),
            ("print *,popcnt(3)\nend\n", "popcnt"),
            ("integer :: i,a(2)\ndo concurrent(i=1:2)\na(i)=i\nend do\nend\n", "DO CONCURRENT"),
            ("module m\ncontains\nimpure elemental integer function f(x)\ninteger,intent(in)::x\nf=x\nend function\nend module\n", "IMPURE"),
        ]
        for source,feature in cases:
            with self.subTest(feature=feature): self.assert_feature(source,"f2003","f2008",feature)

    def test_later_revisions(self):
        self.assert_feature("call random_init(.true.,.false.)\nend\n","f2008","f2018","random_init")
        self.assert_feature("print *,sinpi(0.5)\nend\n","f2018","f2023","sinpi")
        self.assert_feature("print *,out_of_range(1.0,0)\nend\n","f2008","f2018","out_of_range")
        self.assert_feature("pure subroutine s()\nerror stop 1\nend subroutine\n","f2008","f2018","ERROR STOP in a PURE")

    def test_comments_strings_continuations_and_names(self):
        source='''implicit none
! [integer::] PURE FORALL ERROR STOP
integer :: pure,elemental,forall
integer :: a(2)
pure=1
elemental=2
forall=3
a=( / pure, & ! continuation
 & elemental / )
print *,"[character(3)::]",sum(a),forall
end
'''
        result=self.invoke(source,"f90",command="check")
        self.assertEqual(result.returncode,0,result.stderr)
        source="integer :: a(2)\na=[1, & ! comment\n & 2]\nend\n"
        self.assert_feature(source,"f95","f2003","square-bracket")

    def test_f90_allocatable_arrays_and_user_module_names(self):
        source='''module iso_fortran_env
integer,parameter :: k=4
end module
program main
use iso_fortran_env
implicit none
type t
integer :: value
end type
integer,allocatable :: numbers(:)
type(t),allocatable :: objects(:)
allocate(numbers(2),objects(2))
numbers=1
objects(1)%value=k
print *,sum(numbers),objects(1)%value
end program
'''
        result=self.invoke(source,"f90",command="check")
        self.assertEqual(result.returncode,0,result.stderr)

    def test_user_procedures_are_not_mistaken_for_new_intrinsics(self):
        source='''module m
contains
real function sinpi(x) result(y)
real,intent(in)::x
y=x
end function
end module
program main
use m
print *,sinpi(1.0)
end program
'''
        result=self.invoke(source,"f90",command="check")
        self.assertEqual(result.returncode,0,result.stderr)

    def test_unrelated_procedure_does_not_hide_intrinsic(self):
        source='''module m
contains
real function sinpi(x) result(y)
real,intent(in)::x
y=x
end function
end module
program main
print *,sinpi(1.0)
end program
'''
        self.assert_feature(source,"f2018","f2023","sinpi")

    def test_obsolescence_and_werror(self):
        source="integer :: a(2),i\nforall(i=1:2) a(i)=i\nend\n"
        result=self.invoke(source,"f2018")
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn("warning: FORALL is obsolescent",result.stderr)
        result=self.invoke(source,"f2018",extra=("-Werror",))
        self.assertNotEqual(result.returncode,0)
        self.assertIn("error: FORALL is obsolescent",result.stderr)
        result=self.invoke(source)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertNotIn("obsolescent",result.stderr)

    def test_option_validation_and_positions(self):
        source="print *,1\nend\n"
        for option in ("-std=f77","-std=f2099"):
            result=self.invoke(source,extra=(option,))
            self.assertNotEqual(result.returncode,0)
            self.assertIn("possible values",result.stderr)
        path=self.work / "source.f90"
        path.write_text(source)
        for flags in (("--std","f90"),("-std","f90"),("-std=f90",)):
            result=subprocess.run([str(COMPILER),"check",str(path),*flags],cwd=self.work,capture_output=True,text=True,timeout=30)
            self.assertEqual(result.returncode,0,result.stderr)

    def test_multifile_and_imported_module_diagnostics(self):
        module=self.work / "m.f90"
        module.write_text("module m\ninteger,parameter :: values(2)=[1,2]\nend module\n")
        main=self.work / "main.f90"
        main.write_text("use m\nprint *,sum(values)\nend\n")
        result=subprocess.run([str(COMPILER),"-std=f95",str(module),str(main),"-o",str(self.work / "main.exe")],cwd=self.work,capture_output=True,text=True,timeout=60)
        self.assertNotEqual(result.returncode,0)
        self.assertIn("m.f90:2:",result.stderr)
        self.assertIn("square-bracket",result.stderr)
        built=subprocess.run([str(COMPILER),"-c",str(module)],cwd=self.work,capture_output=True,text=True,timeout=60)
        self.assertEqual(built.returncode,0,built.stderr)
        result=subprocess.run([str(COMPILER),"-std=f95","check",str(main)],cwd=self.work,capture_output=True,text=True,timeout=60)
        self.assertNotEqual(result.returncode,0)
        self.assertIn("m.f90:2:",result.stderr)
        self.assertIn("square-bracket",result.stderr)


if __name__=="__main__": unittest.main(verbosity=2)

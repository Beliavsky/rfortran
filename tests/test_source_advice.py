"""Scope, value-flow, status, and storage diagnostics on the source AST."""
import unittest
from pathlib import Path
import subprocess
import tempfile
import test_unused_warnings as shared
from test_record_read import COMPILER


class SourceAdvice(unittest.TestCase):
    check = shared.UnusedWarnings.check

    def test_shadow_and_dummy_are_separate(self):
        source = """program main
implicit none
integer :: x
x=1
call work(2)
contains
subroutine work(x)
integer,intent(in)::x
print *,x
block
integer::x
x=3
print *,x
end block
end subroutine
end program
"""
        warning = self.check(source, ["-Wshadow"])
        self.assertEqual(warning.count("[shadow]"), 1, warning)
        self.assertIn("example.f90:11:", warning)
        self.assertIn("original declaration", warning)
        warning = self.check(source, ["-Wshadow-dummy-argument"])
        self.assertEqual(warning.count("[shadow-dummy-argument]"), 1, warning)
        self.assertIn("dummy argument `x`", warning)
        self.assertNotIn("[shadow]", warning)

    def test_shadow_use_association(self):
        source = """module constants
integer,parameter::n=3
end module
program main
use constants,only:count=>n
implicit none
call work()
contains
subroutine work()
integer::count
count=4
print *,count
end subroutine
end program
"""
        warning = self.check(source, ["-Wshadow"])
        self.assertIn("local declaration `count`", warning)
        self.assertIn("integer,parameter::n=3", warning)

    def test_dead_store(self):
        source = """implicit none
real::x,y
x=1.1
y=1.2
x=1.3
print *,x,y
end
"""
        warning = self.check(source, ["-Wdead-store"])
        self.assertEqual(warning.count("[dead-store]"), 1, warning)
        self.assertIn("example.f90:3:1", warning)
        self.assertIn("overwriting assignment", warning)
        self.assertNotIn("[dead-store]", self.check(source, ["-Wall"]))
        self.assertEqual(self.check(source, ["-Wdead-store", "-Wno-dead-store"]), "")

    def test_dead_store_read_modify_branch_and_call(self):
        source = """program main
implicit none
integer::x
x=1
x=x+1
call observe()
x=3
if(x>0)then
x=4
end if
x=5
print *,x
contains
subroutine observe()
print *,x
end subroutine
end program
"""
        self.assertEqual(self.check(source, ["-Wdead-store"]), "")

    def test_dead_store_excludes_persistent_and_dummy_storage(self):
        source = """program main
implicit none
integer::y
y=0
call work(y)
print *,y
contains
subroutine work(y)
integer,intent(inout)::y
integer,save::x
integer,target::z
x=1
x=2
y=1
y=2
z=1
z=2
print *,x,z
end subroutine
end program
"""
        self.assertEqual(self.check(source, ["-Wdead-store"]), "")

    def test_integer_division(self):
        source = """implicit none
integer,parameter::n=5
print *,n/2,6/2,-5/2,5.0/2
end
"""
        warning = self.check(source, ["-Winteger-division"])
        self.assertEqual(warning.count("[integer-division]"), 2, warning)
        self.assertIn("truncates its result to 2", warning)
        self.assertIn("truncates its result to -2", warning)
        self.assertIn("example.f90:3:", warning)
        self.assertIn("[integer-division]", self.check(source, ["-Wall"]))
        self.assertEqual(self.check(source, ["-Winteger-division", "-Wno-integer-division"]), "")
        self.assertIn("error:", self.check(source, ["-Werror=integer-division"], errors=True))

    def test_integer_division_skips_type_inquiry(self):
        self.assertEqual(self.check("implicit none\nprint *,kind(5/2)\nend\n", ["-Winteger-division"]), "")

    def test_integer_division_continued_location(self):
        warning = self.check("print *,6/2,kind(7/2), &\n 5/2\nend\n", ["-Winteger-division"])
        self.assertEqual(warning.count("[integer-division]"), 1, warning)
        self.assertIn("example.f90:2:3", warning)

    def test_integer_division_user_intrinsic_name(self):
        source = """program main
print *,abs(5)/2
contains
integer function abs(n)
integer,intent(in)::n
abs=n+1
end function
end program
"""
        self.assertEqual(self.check(source, ["-Winteger-division"]), "")

    def test_status_checked_and_overwritten(self):
        source = """implicit none
integer::ios,x
character(10)::s
s='1'
read(s,*,iostat=ios)x
read(s,*,iostat=ios)x
if(ios/=0)stop 1
print *,x
end
"""
        warning = self.check(source, ["-Wunchecked-status"])
        self.assertEqual(warning.count("[unchecked-status]"), 1, warning)
        self.assertIn("example.f90:5:", warning)
        self.assertNotIn("[unchecked-status]", self.check(source, ["-Wall"]))

    def test_status_used_in_parent_tail(self):
        source = """implicit none
integer::ios,x
character(10)::s
s='1'
ios=0
if(len_trim(s)>0)then
read(s,*,iostat=ios)x
end if
if(ios/=0)stop 1
end
"""
        self.assertEqual(self.check(source, ["-Wunchecked-status"]), "")

    def test_status_used_by_loop_condition(self):
        source = """implicit none
integer::ios,x
character(10)::s
s='1'
ios=0
do while(ios==0)
read(s,*,iostat=ios)x
end do
end
"""
        self.assertEqual(self.check(source, ["-Wunchecked-status"]), "")

    def test_allocation_status(self):
        source = """implicit none
integer::ierr
real,allocatable::x(:)
allocate(x(2),stat=ierr)
x=1
print *,x
deallocate(x)
end
"""
        warning = self.check(source, ["-Wunchecked-status"])
        self.assertEqual(warning.count("[unchecked-status]"), 1, warning)
        self.assertIn("stat result `ierr`", warning)

    def test_command_status(self):
        source = """implicit none
integer::exit_status,command_status
call execute_command_line('echo hello',exitstat=exit_status,cmdstat=command_status)
print *,command_status
end
"""
        # Advice is available even where native command keyword lowering is
        # still unsupported. Werror stops before that independent limitation.
        warning = self.check(source, ["-Werror=unchecked-status"], errors=True)
        self.assertEqual(warning.count("[unchecked-status]"), 1, warning)
        self.assertIn("exitstat result `exit_status`", warning)

    def test_aliasing_and_keyword_arguments(self):
        source = """program main
implicit none
integer::x
x=2
call change(b=x,a=x)
print *,x
contains
subroutine change(a,b)
integer,intent(in)::a
integer,intent(out)::b
b=a+1
end subroutine
end program
"""
        warning = self.check(source, ["-Waliasing"])
        self.assertEqual(warning.count("[aliasing]"), 1, warning)
        self.assertIn("`a` and `b`", warning)
        self.assertIn("example.f90:5:", warning)
        self.assertEqual(self.check(source.replace("b=x,a=x", "b=x,a=(x+0)"), ["-Waliasing"]), "")
        self.assertEqual(self.check(source.replace("b=x,a=x", "b=x,a=(x)"), ["-Waliasing"]), "")

    def test_aliasing_disjoint_elements(self):
        source = """program main
implicit none
integer::x(2)
x=2
call change(x(1),x(2))
print *,x
contains
subroutine change(a,b)
integer,intent(in)::a
integer,intent(out)::b
b=a+1
end subroutine
end program
"""
        self.assertEqual(self.check(source, ["-Waliasing"]), "")
        warning = self.check(source.replace("x(1),x(2)", "x(1),x(1)"), ["-Waliasing"])
        self.assertEqual(warning.count("[aliasing]"), 1, warning)

    def test_pointer_local_target_escapes_to_result(self):
        # Werror stops before pointer-result backend limitations; check source advice.
        source = """program main
implicit none
contains
function bad() result(p)
integer,pointer::p
integer,target::x
x=1
p=>x
end function
end program
"""
        warning = self.check(source, ["-Werror=target-lifetime"], errors=True)
        self.assertIn("pointer `p` may outlive automatic target `x`", warning)
        self.assertIn("example.f90:8:", warning)
        self.assertIn("target ceases to exist", warning)

    def test_safe_pointer_association(self):
        source = """implicit none
integer,target::x
integer,pointer::p
x=1
p=>x
print *,p
end
"""
        self.assertEqual(self.check(source, ["-Wtarget-lifetime"]), "")

    def test_directives(self):
        source = "!#allow(integer-division)\nprint *,5/2\nend\n"
        self.assertNotIn("[integer-division]", self.check(source, ["-Wall"]))
        self.assertIn("[integer-division]", self.check(source.replace("allow", "deny"), errors=True))

    def test_integer_division_kinds_and_parameters(self):
        source = """implicit none
integer,parameter::wide=selected_int_kind(12),n=7
integer(wide),parameter::q=n/2
real,parameter::r=7.0
print *,q,r/2,9_wide/2_wide
end
"""
        warning = self.check(source, ["-Winteger-division"])
        self.assertEqual(warning.count("[integer-division]"), 2, warning)

    def test_external_procedure_has_no_main_host(self):
        source = """program main
integer::x
x=1
call work()
print *,x
end program
subroutine work()
integer::x
x=2
print *,x
end subroutine
"""
        self.assertEqual(self.check(source, ["-Wshadow"]), "")

    def test_status_forwarded_or_observable(self):
        source = """program main
integer::ios
call work(ios)
print *,ios
contains
subroutine work(ios)
integer,intent(out)::ios
integer::x
character(10)::s
s='1'
read(s,*,iostat=ios)x
end subroutine
end program
"""
        self.assertEqual(self.check(source, ["-Wunchecked-status"]), "")

    def test_command_argument_status(self):
        source = """implicit none
integer::ios
character(10)::s
call get_command_argument(1,s,status=ios)
print *,s
end
"""
        warning = self.check(source, ["-Wunchecked-status"])
        self.assertEqual(warning.count("[unchecked-status]"), 1, warning)
        self.assertIn("status result `ios`", warning)
        warning = self.check(source.replace("print *,s", "print *,s,ios"), ["-Wunchecked-status"])
        self.assertEqual(warning, "")

    def test_formatted_read_and_write_status(self):
        source = """implicit none
integer::ios,x
character(10)::s
s='1'
read(s,'(i1)',iostat=ios)x
write(*,'(i1)',iostat=ios)x
end
"""
        # WRITE's IOSTAT lowering is still limited; diagnose before lowering.
        warning = self.check(source, ["-Werror=unchecked-status"], errors=True)
        self.assertEqual(warning.count("[unchecked-status]"), 2, warning)

    def test_file_control_status(self):
        source = """implicit none
integer::u,ios
logical::opened
open(newunit=u,status='scratch',iostat=ios)
rewind(u,iostat=ios)
inquire(unit=u,opened=opened,iostat=ios)
close(u,iostat=ios)
end
"""
        warning = self.check(source, ["-Wunchecked-status"])
        self.assertEqual(warning.count("[unchecked-status]"), 4, warning)

    def test_aliasing_sections(self):
        source = """program main
implicit none
integer::x(6)
x=2
call change(x(1:3),x(4:6))
print *,x
contains
subroutine change(a,b)
integer,intent(in)::a(:)
integer,intent(out)::b(:)
b=a+1
end subroutine
end program
"""
        self.assertEqual(self.check(source, ["-Waliasing"]), "")
        warning = self.check(source.replace("x(1:3),x(4:6)", "x(1:3),x(3:5)"), ["-Waliasing"])
        self.assertEqual(warning.count("[aliasing]"), 1, warning)
        # Empty sections cannot overlap a whole array. No native shape check
        # should be reached in this deliberately empty assignment test.
        quiet = self.check(source.replace("x(1:3),x(4:6)", "x(2:1),x(2:1)"), ["-Waliasing"])
        self.assertEqual(quiet, "")

    def test_aliasing_read_only_inout(self):
        source = """program main
integer::x
x=2
call observe(x,x)
contains
subroutine observe(a,b)
integer,intent(inout)::a,b
print *,a,b
end subroutine
end program
"""
        self.assertEqual(self.check(source, ["-Waliasing"]), "")

    def test_pointer_escape_through_alias(self):
        source = """program main
implicit none
contains
function bad() result(p)
integer,pointer::p,q
integer,target::x
x=1
q=>x
p=>q
end function
end program
"""
        warning = self.check(source, ["-Werror=target-lifetime"], errors=True)
        self.assertIn("pointer `p` may outlive automatic target `x`", warning)

    def test_pointer_block_target(self):
        source = """implicit none
integer,pointer::p
block
integer,target::x
x=1
p=>x
end block
end
"""
        warning = self.check(source, ["-Werror=target-lifetime"], errors=True)
        self.assertIn("pointer `p` may outlive automatic target `x`", warning)

    def test_saved_target_does_not_escape_automatic_storage(self):
        source = """program main
implicit none
contains
function safe() result(p)
integer,pointer::p
integer,target,save::x
x=1
p=>x
end function
end program
"""
        # Native pointer function results are unsupported; absence of this
        # warning is checked independently of the later backend diagnostic.
        warning = self.check(source, ["-Wtarget-lifetime"], errors=True)
        self.assertNotIn("[target-lifetime]", warning)

    def test_all_advice_enabled_together(self):
        source = """program main
implicit none
integer::x
x=1
x=3/2
call change(x,x)
print *,x
contains
subroutine change(a,b)
integer,intent(in)::a
integer,intent(out)::b
b=a+1
end subroutine
end program
"""
        flags = ["-Wall", "-Wshadow", "-Wshadow-dummy-argument", "-Wdead-store", "-Wunchecked-status"]
        warning = self.check(source, flags)
        for category in ["dead-store", "integer-division", "aliasing"]:
            self.assertIn("[" + category + "]", warning)

    def test_multifile_and_separate_module_locations(self):
        with tempfile.TemporaryDirectory(prefix="source-advice-") as tmp:
            directory = Path(tmp)
            module = directory / "constants.f90"
            main = directory / "main.f90"
            module.write_text("module constants\ninteger,parameter::count=3\nend module\n")
            main.write_text("""program main
use constants
call work()
contains
subroutine work()
integer::count
count=4
print *,count
end subroutine
end program
""")
            flags = ["-Werror=shadow", "-c"]
            result = subprocess.run([COMPILER, *flags, module, main], cwd=directory,
                                    capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(result.stderr.count("[shadow]"), 1, result.stderr)
            self.assertIn("main.f90:6:", result.stderr)
            self.assertIn("constants.f90:2:", result.stderr)
            result = subprocess.run([COMPILER, "-c", module], cwd=directory,
                                    capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            result = subprocess.run([COMPILER, "-Werror=shadow", "check", main], cwd=directory,
                                    capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(result.stderr.count("[shadow]"), 1, result.stderr)
            self.assertIn("main.f90:6:", result.stderr)
            self.assertIn("constants.f90:2:", result.stderr)


if __name__ == "__main__":
    unittest.main()

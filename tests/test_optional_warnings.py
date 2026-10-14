"""Presence warnings on source procedures, before OPTIONAL specialization."""
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER
import test_unused_warnings

VEC=(ROOT/'tests/183_optional_character_array_result.f90').read_text(encoding='utf-8')

class OptionalWarnings(unittest.TestCase):
    check=test_unused_warnings.UnusedWarnings.check

    def test_vec_controls_and_location(self):
        self.assertEqual(self.check(VEC),'')
        warning=self.check(VEC,['-Woptional-argument'])
        self.assertIn('OPTIONAL argument `s1` of procedure `vec`',warning)
        self.assertIn('str(1)=s1',warning)
        self.assertEqual(warning.count('warning:'),45,warning)
        self.assertEqual(self.check(VEC,['-Woptional-argument','-Wno-optional-argument']),'')
        self.assertEqual(self.check(VEC,['-Wall','-Wno-unused','-Wno-optional-argument']),'')
        self.assertIn('[optional-argument]',self.check(VEC,['-Wall','-Wno-unused']))
        self.assertIn('[optional-argument]',self.check(VEC,['-Wno-optional-argument','-Woptional-argument']))

    def test_warning_errors(self):
        self.assertEqual(self.check(VEC,['-Werror']),'')
        self.assertIn('error: OPTIONAL',self.check(VEC,['-Werror=optional-argument'],errors=True))
        self.assertIn('error: OPTIONAL',self.check(VEC,['-Woptional-argument','-Werror'],errors=True))
        self.assertEqual(self.check(VEC,['-Werror=optional-argument','-Wno-optional-argument']),'')

    def test_present_branches_and_early_return(self):
        source='''program main
call work(7,8)
contains
subroutine work(x,y)
integer,optional,intent(in)::x,y
if(present(x))then
print *,x
end if
if(.not.present(x))then
print *,0
else
print *,x
end if
if(.not.present(x))return
print *,x
if(present(y))then
print *,x,y
end if
end subroutine work
end program main
'''
        self.assertEqual(self.check(source,['-Woptional-argument']),'')

    def test_conjunction_guards_and_loop(self):
        self.assertEqual(self.check('''program main
call work(1,2)
contains
subroutine work(x,y)
integer,optional,intent(in)::x,y
if(present(x).and.present(y))then
print *,x,y
end if
if(.not.present(x).or..not.present(y))return
do while(present(x))
print *,x,y
exit
end do
end subroutine work
end program main
''',['-Woptional-argument']),'')

    def test_logical_evaluation_is_not_assumed_short_circuit(self):
        warning=self.check('''program main
call work(1)
contains
subroutine work(x)
integer,optional,intent(in)::x
if(present(x).and.x>0)print *,x
end subroutine work
end program main
''',['-Woptional-argument'])
        self.assertEqual(warning.count('warning:'),1,warning)
        self.assertIn('example.f90:6:19',warning)

    def test_presence_does_not_leak_out_of_conditional(self):
        warning=self.check('''program main
call work(7)
contains
subroutine work(x)
integer,optional,intent(in)::x
if(present(x))then
print *,x
end if
print *,x
end subroutine work
end program main
''',['-Woptional-argument'])
        self.assertEqual(warning.count('warning:'),1,warning)
        self.assertIn('example.f90:9:9',warning)

    def test_forwarding_whole_dummy_and_required_argument(self):
        warning=self.check('''program main
call work(7)
contains
subroutine work(x)
integer,optional,intent(in)::x
call accept(x)
call accept(y=x)
call required(x)
end subroutine work
subroutine accept(y)
integer,optional,intent(in)::y
if(present(y))print *,y
end subroutine accept
subroutine required(y)
integer,intent(in)::y
print *,y
end subroutine required
end program main
''',['-Woptional-argument'])
        self.assertEqual(warning.count('warning:'),1,warning)
        self.assertIn('example.f90:8:',warning)

    def test_forwarded_section_still_requires_presence(self):
        warning=self.check('''program main
call work([1,2])
contains
subroutine work(x)
integer,optional,intent(in)::x(:)
call accept(x(:))
end subroutine work
subroutine accept(y)
integer,optional,intent(in)::y(:)
if(present(y))print *,y
end subroutine accept
end program main
''',['-Woptional-argument'])
        self.assertEqual(warning.count('warning:'),1,warning)
        self.assertIn('example.f90:6:',warning)

    def test_intrinsic_optional_forwarding(self):
        self.assertEqual(self.check('''program main
integer::n
call work(n)
contains
subroutine work(n)
integer,optional,intent(out)::n
call random_seed(size=n)
end subroutine work
end program main
''',['-Woptional-argument']),'')

    def test_guard_inside_block_and_keyword_present(self):
        self.assertEqual(self.check('''program main
call work(7)
contains
subroutine work(x)
integer,optional,intent(in)::x
if(present(a=x))print *,x
block
if(.not.present(x))return
end block
print *,x
end subroutine work
end program main
''',['-Woptional-argument']),'')

    def test_writes_and_array_elements_require_presence(self):
        warning=self.check('''program main
integer::a(2)
a=0
call work(a)
contains
subroutine work(x)
integer,optional,intent(inout)::x(:)
x(1)=3
read(*,*)x(2)
end subroutine work
end program main
''',['-Woptional-argument'])
        self.assertEqual(warning.count('warning:'),2,warning)
        self.assertIn('example.f90:8:',warning)
        self.assertIn('example.f90:9:',warning)

    def test_shadowing_and_source_directives(self):
        source='''program main
call work(7)
contains
subroutine work(x)
integer,optional,intent(in)::x
block
integer::x
x=9
print *,x
end block
if(present(x))print *,x
end subroutine work
end program main
'''
        self.assertNotIn('[optional-argument]',self.check(source,['-Woptional-argument']))
        self.assertEqual(self.check('!#allow(optional-argument)\n'+VEC,['-Wall','-Wno-unused']),'')
        self.assertIn('error: OPTIONAL',self.check('!#deny(optional-argument)\n'+VEC,errors=True))

    def test_multifile_use_rename_and_original_source_location(self):
        with tempfile.TemporaryDirectory(prefix='optional-warnings-') as tmp:
            directory=Path(tmp)
            module=directory/'m.f90'
            module.write_text('''module m
contains
subroutine accept(y)
integer,optional,intent(in)::y
if(present(y))print *,y
end subroutine accept
end module m
''',encoding='utf-8')
            main=directory/'main.f90'
            main.write_text('''program main
use m,only:take=>accept
call work(7)
contains
subroutine work(x)
integer,optional,intent(in)::x
call take(y=x)
print *,x
end subroutine work
end program main
''',encoding='utf-8')
            result=subprocess.run([COMPILER,'-Woptional-argument',module,main,'-o',directory/'main.exe'],capture_output=True,text=True,timeout=60)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stderr.count('warning:'),1,result.stderr)
            self.assertIn('main.f90:8:9',result.stderr)
            self.assertNotIn('.f90c-sources.f90',result.stderr)

if __name__=='__main__':
    unittest.main()

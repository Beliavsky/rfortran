"""Scalar CHARACTER function buffers, dependent lengths, and allocatable results."""
from pathlib import Path
import unittest
import subprocess
import tempfile
import test_deallocate
from test_record_read import ROOT, build_commands


class CharacterFunctions(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_original_program(self):
        self.check((ROOT / "tests/146_character_functions.f90").read_text(encoding="utf-8"),
                   "'abc def'\n'abc def'\n")

    def test_fixed_lengths_named_nested_calls_and_returns(self):
        self.check("""module m
contains
pure function wrap(text,flag) result(out)
character(len=*),intent(in)::text
logical,intent(in)::flag
character(len=len(text)+2)::out
if(flag)then
out='['//text//']'
return
end if
out='('//text//')'
end function wrap
pure character(len=2) function short(text)
character(len=*),intent(in)::text
short=text
end function short
pure function sized(text) result(out)
character(2),intent(in)::text
character(len=len_trim(text))::out
out=text
end function sized
end module m
program main
use m
character(len=4)::text
text='abc'
print '(a)',wrap(text,.true.)
print '(a)',wrap(flag=.false.,text=short('abcdef'))
print '(a)',wrap(short(wrap('abc',.true.)),.true.)
print *,len(wrap('abc',.false.))
print '(a)',sized('abcdef')
end program main
""", "[abc ]\n(ab)\n[[a]\n5\nab\n")

    def test_allocatable_runtime_result_lengths(self):
        self.check("""program main
character(len=5)::a,b
character(len=:),allocatable::out
read(*,'(a)')a
read(*,'(a)')b
out=join(a,b)
print '(a)',"'"//out//"'"
print *,len(out),len(join(a,b))
out=join('','')
print *,len(out)
contains
pure function join(a,b) result(out)
character(len=*),intent(in)::a,b
character(len=:),allocatable::out
out=trim(a)//trim(b)
end function join
end program main
""", "'abc def'\n7 7\n0\n", data="abc  \n def \n")

    def test_no_argument_impure_function_and_single_evaluation(self):
        self.check("""program main
character(6)::text
text=value()//value()
print '(a)',text
contains
function value() result(out)
character(3)::out
print '(a)','called'
out='x'
end function value
end program main
""", "called\ncalled\nx  x  \n")

    def test_unused_environment_function_definition(self):
        program="""module m
contains
function exe_name() result(xname)
character(len=1000)::xname,dir_name
call getcwd(dir_name)
call get_command_argument(0,xname)
xname=trim(dir_name)//'/'//trim(xname)
end function exe_name
end module m
program main
use m
print '(a)','ok'
end program main
"""
        # GETCWD is a GNU extension, disabled by the reference helper's -std=f2018.
        with tempfile.TemporaryDirectory(prefix="character-function-unused-") as tmp:
            directory=Path(tmp);source=directory/'example.f90'
            source.write_text(program,encoding='utf-8')
            for command in build_commands(source,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stderr,"")
                    self.assertEqual(result.stdout,"ok\n")

    def test_optional_character_inputs(self):
        self.check("""program main
print '(a)','['//label('a')//']'
print '(a)','['//label(suffix='!',text='a')//']'
contains
function label(text,suffix) result(out)
character(len=*),intent(in)::text
character(1),intent(in),optional::suffix
character(len=len(text)+1)::out
out=text
if(present(suffix))out=trim(text)//suffix
end function label
end program main
""", "[a ]\n[a!]\n")

    def test_writable_scalar_actual(self):
        self.check("""program main
integer::x
x=1
print *,change(x)
print *,x
contains
function change(x) result(out)
integer,intent(inout)::x
character(1)::out
x=x+1
out='a'
end function change
end program main
""", "a\n2\n")

    def test_diagnostics(self):
        self.reject("""program main
print *,copy(['a','b'])
contains
pure function copy(text) result(out)
character(len=*),intent(in)::text
character(len=len(text))::out
out=text
end function copy
end program main
""", "non-elemental CHARACTER function `copy` currently requires scalar actuals")
        self.reject("""program main
print *,bad('a')
contains
function bad(text) result(out)
character(1),intent(in)::text
character(1)::out
text='b'
out=text
end function bad
end program main
""", "cannot modify INTENT(IN) dummy `text`")
        self.reject("""program main
print *,again('a')
contains
recursive function again(text) result(out)
character(len=*),intent(in)::text
character(len=len(text))::out
out=again(text)
end function again
end program main
""", "recursive CHARACTER function")


if __name__ == "__main__":
    unittest.main()

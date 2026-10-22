"""Numeric/LOGICAL scalar functions with read-only CHARACTER scalar inputs."""
import unittest
import json
import subprocess
import tempfile
from pathlib import Path
import test_deallocate
from test_record_read import ROOT, build_commands

class ScalarCharacterArguments(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_original_assert_eq2(self):
        self.check((ROOT/'tests/155_scalar_character_arguments.f90').read_text(encoding='utf-8'),
                   '3\n4\n')

    def test_existing_fixed_length_function_without_intent(self):
        fixture=ROOT/'tests/64_generic_derived.f90'
        expected=json.loads((ROOT/'tests/expectations.json').read_text(encoding='utf-8'))['cases'][fixture.stem]['program_stdout']
        self.check(fixture.read_text(encoding='utf-8'),expected)

    def test_failed_assertion_stops_before_result_is_read(self):
        source=(ROOT/'tests/155_scalar_character_arguments.f90').read_text(encoding='utf-8')
        source=source.replace("print *, assert_eq2(3, 3, 'first check')", "call trigger()")
        source=source.replace('contains\nfunction', '''contains
subroutine trigger()
integer::result
result=assert_eq2(3,4,'first check')
print *,result
end subroutine trigger
function''')
        self.stop_check(source,'nrerror: an assert_eq failed with this tag:first check',
                        'STOP program terminated by assert_eq2',0)

    def test_runtime_assertion_branches(self):
        source=(ROOT/'tests/155_scalar_character_arguments.f90').read_text(encoding='utf-8')
        source=source.replace("print *, assert_eq2(3, 3, 'first check')", '''integer::value
character(12)::label
read(*,*)value
read(*,'(a)')label
print *,assert_eq2(value,7,label)''')
        self.check(source,'7\n4\n',data='7\nruntime tag\n')
        self.stop_check(source,'nrerror: an assert_eq failed with this tag:runtime tag',
                        'STOP program terminated by assert_eq2',0,data='8\nruntime tag\n')

    def stop_check(self,source,stdout,stderr,code,data=''):
        with tempfile.TemporaryDirectory(prefix='scalar-character-stop-') as tmp:
            directory=Path(tmp)
            path=directory/'example.f90'
            path.write_text(source,encoding='utf-8')
            for command in build_commands(path,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,input=data,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,code,result.stderr)
                    self.assertEqual(result.stdout.strip(),stdout)
                    self.assertEqual(result.stderr.strip(),stderr)

    def test_integer_stop_code_and_pure_restriction(self):
        self.stop_check("print '(a)','before'\nstop 7\nprint *,'after'\nend\n",'before','STOP 7',7)
        self.reject("""print *,bad('abc')
contains
pure integer function bad(text)
character(len=*),intent(in)::text
stop 'bad'
bad=len(text)
end function bad
end
""",'PURE procedure cannot execute STOP')

    def test_runtime_text_pure_typed_results_and_nested_calls(self):
        self.check("""program main
character(5)::text
read(*,'(a)')text
print *,length(text),length(''),length(label(text))
print *,score(text),is_match(text,'abc')
print *,short('abcdef')
contains
pure integer function length(text)
character(len=*),intent(in)::text
length=len_trim(text)
end function length
pure real function score(text)
character(len=*),intent(in)::text
score=real(len_trim(text))*0.5
end function score
pure logical function is_match(text,target)
character(len=*),intent(in)::text,target
is_match=text==target
end function is_match
pure function label(text) result(out)
character(len=*),intent(in)::text
character(len=len(text)+2)::out
out='['//text//']'
end function label
integer function short(text)
character(2),intent(in)::text
short=len(text)
end function short
end program main
""",'3 0 7\n1.5 T\n2\n',data='abc\n')

    def test_numeric_array_input_and_scalar_character_tag(self):
        self.check("""program main
print *,tagged_sum([1,2,3],'sum')
contains
function tagged_sum(values,tag) result(out)
integer,intent(in)::values(:)
character(len=*),intent(in)::tag
integer::out
out=sum(values)+len(tag)
end function tagged_sum
end program main
""",'9\n')

    def test_early_return_and_optional_scalar_character(self):
        self.check("""program main
print *,measure(),measure('abc'),measure('')
contains
integer function measure(text) result(out)
character(len=*),intent(in),optional::text
if(.not.present(text))then
out=-1
return
end if
out=len(text)
end function measure
end program main
""",'-1 3 0\n')

    def test_invalid_types_unset_inputs_and_readonly_writes(self):
        for decl,actual,message in [
            ('','1','type mismatch for dummy'),
            ('character(3)::text\n','text','uninitialized variable'),
        ]:
            self.reject('program main\n'+decl+'print *,length('+actual+')\ncontains\ninteger function length(text)\ncharacter(len=*),intent(in)::text\nlength=len(text)\nend function length\nend program main\n',message)
        self.reject("""print *,bad('abc')
contains
integer function bad(text)
character(len=*),intent(in)::text
text='xyz'
bad=1
end function bad
end
""",'INTENT(IN)')

if __name__ == '__main__':
    unittest.main()

"""Runtime CHARACTER stop codes in interpreted and native execution."""
from pathlib import Path
import unittest
import subprocess
import tempfile
from test_record_read import build_commands
import test_deallocate as helpers

class RuntimeErrorStop(unittest.TestCase):
    check = helpers.Deallocate.check
    reject = helpers.Deallocate.reject

    def test_array_element_repro(self):
        source = (Path(__file__).parent / 'programs/xerror_stop_character.f90').read_text()
        self.check(source, error='ERROR STOP rename_cols: column not found: missing')

    def test_variable_and_substring(self):
        self.check("character(8)::s='message!'\ninteger::i\ni=3\nerror stop s(i:7)\nend\n", error='ERROR STOP ssage')

    def test_pure_procedure(self):
        self.check("""program main
call fail('missing')
contains
pure subroutine fail(key)
character(*),intent(in)::key
error stop 'not found: '//trim(key)
end subroutine
end program
""", error='ERROR STOP not found: missing')

    def test_function_result(self):
        self.check("""program main
error stop message()
contains
function message() result(s)
character(6)::s
s='failed'
end function
end program
""", error='ERROR STOP failed')

    def test_empty_and_unexecuted(self):
        self.check("character(0)::s\ns=''\nerror stop s\nend\n", error='ERROR STOP ')
        self.check("character(8)::s='unused'\nlogical::fail\nfail=.false.\nif(fail) error stop trim(s)\nprint *,1\nend\n", '1\n')

    def test_exact_message_and_status(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            source = directory / 'example.f90'
            source.write_text("character(4)::s='bad'\nerror stop s\nend\n")
            for command in build_commands(source, directory):
                with self.subTest(command=command):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertEqual(result.stdout, '')
                    self.assertEqual(result.stderr, 'ERROR STOP bad \n')

    def test_invalid_type(self):
        self.reject('error stop 1.5\nend\n', 'ERROR STOP')

if __name__ == '__main__':
    unittest.main()

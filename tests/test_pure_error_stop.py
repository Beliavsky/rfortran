"""ERROR STOP is permitted in PURE procedures; STOP remains forbidden."""
import subprocess
import tempfile
import unittest
from pathlib import Path
import test_deallocate
from test_record_read import ROOT,build_commands

class PureErrorStop(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_compress_reproducer(self):
        self.check((ROOT/'xpure_error_stop.f90').read_text(encoding='utf-8'),'1 1\n')

    def test_failed_size_check(self):
        source=(ROOT/'xpure_error_stop.f90').read_text(encoding='utf-8')
        self.check(source.replace('call compress(2, factors)','call compress(3, factors)'),
                   error='ERROR STOP in compress, incorrect factors size')

    def test_unconditional_error_stop_does_not_require_a_result(self):
        self.check("""program main
implicit none
print *, fail()
contains
pure integer function fail() result(j)
error stop 'no result'
end function
end program
""",error='ERROR STOP no result')

    def test_elemental_function_can_error_stop(self):
        self.check("""program main
implicit none
print *,f(2)
contains
elemental integer function f(n) result(j)
integer,intent(in)::n
if(n<0) error stop 'negative input'
j=n+1
end function
end program
""",'3\n')

    def test_integer_error_stop_exit_status(self):
        source="""program main
call fail()
contains
pure subroutine fail()
error stop 7
end subroutine
end program
"""
        with tempfile.TemporaryDirectory() as directory:
            directory=Path(directory)
            path=directory/'example.f90'
            path.write_text(source,encoding='utf-8')
            for command in build_commands(path,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,encoding='utf-8')
                    self.assertEqual(result.returncode,7,result.stderr)
                    self.assertEqual(result.stdout,'')
                    self.assertIn('ERROR STOP 7',result.stderr)

    def test_stop_and_impure_error_codes_remain_rejected(self):
        self.reject('pure subroutine f()\nstop\nend subroutine\n','cannot execute STOP')
        self.reject("pure subroutine f()\nstop 'bad'\nend subroutine\n",'cannot execute STOP')
        self.reject("""pure subroutine f()
error stop code()
end subroutine
integer function code()
code=7
end function
""",'impure or unknown procedure `code`')

if __name__=='__main__':unittest.main()

"""OPTIONAL elemental calls, argument forwarding, and integer ERROR STOP."""
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_deallocate
from test_record_read import ROOT, build_commands

class OptionalElemental(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_sgn_real_program(self):
        self.check((ROOT/'tests/144_optional_elemental.f90').read_text(encoding='utf-8'),
                   'All sgn_real tests passed.\n')

    def test_scalar_arrays_optional_arrays_and_absent_forwarding(self):
        self.check("""program main
integer::a(3)
a=adjust([1,2,3])
print *,a
print *,adjust([1,2,3],offset=[10,20,30])
print *,outer(2),outer(2,4),adjust(2,negate=.true.)
contains
elemental integer function adjust(x,offset,negate)
integer,intent(in)::x
integer,intent(in),optional::offset
logical,intent(in),optional::negate
adjust=x
if(present(offset))adjust=adjust+offset
if(present(negate))then
if(negate)adjust=-adjust
end if
end function adjust
elemental integer function outer(x,offset)
integer,intent(in)::x
integer,intent(in),optional::offset
outer=adjust(x,offset)
end function outer
end program main
""", '1 2 3\n11 22 33\n2 6 -2\n')

    def test_character_optional_elemental(self):
        self.check("""program main
character(len=3)::a(2)
a=decorate(['a ','b '])
print '(2(a,1x))',a
a=decorate(['a ','b '], suffix='!')
print '(2(a,1x))',a
contains
elemental function decorate(text,suffix) result(out)
character(len=*),intent(in)::text
character(len=1),intent(in),optional::suffix
character(len=len(text)+1)::out
out=text
if(present(suffix))out=trim(text)//suffix
end function decorate
end program main
""", 'a   b   \na!  b!  \n')

    def test_elemental_result_as_array_actual(self):
        self.check("""program main
call verify(adjust([1,2,3],offset=2))
contains
elemental integer function adjust(x,offset)
integer,intent(in)::x
integer,intent(in),optional::offset
adjust=x
if(present(offset))adjust=adjust+offset
end function adjust
subroutine verify(x)
integer,intent(in)::x(:)
print *,sum(abs(x-[3,4,5]))
end subroutine verify
end program main
""", '0\n')

    def test_absent_reference_and_mismatched_shapes(self):
        routine="""contains
elemental integer function f(x,y)
integer,intent(in)::x
integer,intent(in),optional::y
f=x+y
end function f
end program main"""
        self.reject('program main\nprint *,f([1,2])\n'+routine,'reference to absent OPTIONAL dummy `y`')
        self.reject('program main\nprint *,f([1,2],[3,4,5])\n'+routine,'shapes')

    def test_integer_error_stop_exit_code(self):
        with tempfile.TemporaryDirectory(prefix='optional-elemental-') as tmp:
            directory=Path(tmp);source=directory/'stop.f90'
            source.write_text('program main\nerror stop 7\nend program main\n',encoding='utf-8')
            for command in build_commands(source,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,7,result.stderr)
                    self.assertIn('ERROR STOP 7',result.stderr)

if __name__=='__main__':
    unittest.main()

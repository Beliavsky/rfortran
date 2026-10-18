"""Runtime standard-output units, formats, arrays, ADVANCE, and file routing."""
import unittest
import test_deallocate
from test_record_read import ROOT

class OutputUnits(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_matrix_rows(self):
        self.check((ROOT/'tests/195_output_unit.f90').read_text(),'matrix\n1 3\n2 4\n')

    def test_runtime_unit_from_input(self):
        self.check('''integer::u
read(*,*) u
write(u,*) 'list',7,.true.
write(unit=u,fmt='(i0,1x,a)') 8,'named'
write(u,'(a)') 'character'
end
''','list 7 T\n8 named\ncharacter\n',data='6\n')

    def test_constant_character_formats_remain_supported(self):
        self.check('''character(*),parameter::fmt='('//'(a)'//')'
character(*),parameter::nl=achar(10)
write(6,fmt) 'constant'
print *,len(nl),iachar(nl)
end
''','constant\n1 10\n')

    def test_runtime_formats_arrays_and_empty_lists(self):
        self.check('''integer::u,n,a(4),i
character(32)::fmt
u=6
a=[1,2,3,4]
n=2
fmt='("values",*(1x,i0))'
write(u,trim(fmt)) a(:n),sum(a),a(n:1:-1)
fmt='("empty",*(1x,i0))'
n=0
write(u,fmt) a(:n)
end
''','values 1 2 10 2 1\nempty\n')

    def test_runtime_advance_scalars_arrays_and_implied_do(self):
        self.check('''integer::u,i,a(2)
character(4)::advance
u=6
a=[1,2]
advance='nO'
write(u,'(a)',advance=advance) 'prefix:'
write(u,'(*(i0,1x))',advance=trim(advance)) a
write(u,'(*(i0,1x))',advance=advance) (i,i=3,4)
advance='yEs'
write(u,'(a)',advance=advance) 'end'
end
''','prefix:1 2 3 4 end\n')

    def test_runtime_advance_from_specialized_function(self):
        self.check('''program main
integer::u
u=6
call heading('("header")',u)
write(u,'(a)') 'body'
contains
subroutine heading(fmt,u,advance)
character(*),intent(in)::fmt
integer,intent(in)::u
character(*),intent(in),optional::advance
write(u,fmt,advance=default('yes',advance))
end subroutine heading
function default(def,opt) result(value)
character(*),intent(in)::def
character(*),intent(in),optional::opt
character(len(def))::value
if(present(opt)) then
 value=opt
else
 value=def
end if
end function default
end program main
''','header\nbody\n')

    def test_external_a_format_and_stdout_use_distinct_destinations(self):
        self.check('''integer::u,out
character(20)::line
open(newunit=u,status='scratch')
write(u,'(a)') 'file'
rewind(u)
read(u,'(a)') line
close(u)
out=6
write(out,'(a)') trim(line)
write(out,'(a)') 'first','second'
end
''','file\nfirst\nsecond\n')

    def test_invalid_runtime_unit_and_advance(self):
        # List-directed WRITE to a unit with no connected file fails at runtime.
        self.check("integer::u\nu=17\nwrite(u,*) 'invalid'\nend\n",error='file unit is not connected',reference=False)
        self.check("character(5)::advance\nadvance='maybe'\nwrite(6,'(a)',advance=advance) 'invalid'\nend\n",error="ADVANCE must be 'yes' or 'no'",reference=False)
        self.check("write(17,*) 'invalid'\nend\n",error='file unit is not connected',reference=False)

    def test_empty_character_dummy_with_runtime_loop_bound(self):
        program='''program main
character(4)::words(0)
integer::n
read(*,*) n
call show(words,n)
contains
subroutine show(words,n)
character(*),intent(in)::words(:)
integer,intent(in)::n
integer::i
write(6,'("empty",*(1x,a))') (trim(words(i)),i=1,n)
end subroutine show
end program main
'''
        self.check(program,'empty\n',data='0\n')
        self.check(program,error='index out of bounds',data='1\n',reference=False)

if __name__=='__main__':unittest.main()

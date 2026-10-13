"""Internal formatted WRITE: PURE checks, runtime formats, native O0/O2."""
import unittest
import test_deallocate
from test_record_read import ROOT

class InternalWrite(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_str_real_reproducer(self):
        self.check((ROOT/'xstr_real.f90').read_text(encoding='utf-8'),'1.2500\n1.25\n')

    def test_grouped_format_padding_and_numeric_overflow(self):
        self.check("""implicit none
character(24)::text
character(30)::fmt
fmt="('n=',i3,1x,l1,2(1x,f4.1))"
write(text,fmt) 7,.true.,1.5,2.5
print '(a)', '['//text//']'
write(text,'(i1)') 123
print '(a)', '['//text//']'
write(text,'(a)') 'x'
print *,len_trim(text),text(1:1)
end
""",'[n=  7 T  1.5  2.5       ]\n[*                       ]\n1 x\n')

    def test_pure_writable_character_dummy(self):
        self.check("""program main
implicit none
character(8)::text
call format_number(7,text)
print '(a)', '['//text//']'
contains
pure subroutine format_number(n,text)
integer,intent(in)::n
character(*),intent(out)::text
write(text,'(a,i0)') 'n=',n
end subroutine
end program
""",'[n=7     ]\n')

    def test_array_and_implied_do_items(self):
        self.check("""implicit none
integer::a(3),i
character(12)::text
a=[1,2,3]
write(text,'(3i2)') a
print '(a)', '['//text//']'
write(text,'(3(i1,1x))') (a(i),i=3,1,-1)
print '(a)', '['//text//']'
end
""",'[ 1 2 3      ]\n[3 2 1       ]\n')

    def test_empty_output_list(self):
        self.check("""character(8)::text
write(text,"('hi')")
print '(a)', '['//text//']'
end
""",'[hi      ]\n')

    def test_destination_can_also_be_an_input(self):
        self.check("""character(8)::text
text='old'
write(text,'(a,a)') trim(text),'!'
print '(a)', '['//text//']'
end
""",'[old!    ]\n')

    def test_newline_character_is_data(self):
        self.check("""character(2)::text
write(text,'(a)') achar(10)
print *,iachar(text(1:1)),iachar(text(2:2))
end
""",'10 32\n')

    def test_record_overflow(self):
        self.check("""character(2)::text
write(text,'(i3)') 123
end
""",error='internal WRITE record exceeds CHARACTER destination length')

    def test_invalid_runtime_format_and_multiple_records(self):
        self.check("""character(8)::text,fmt
fmt='(i3)'
write(text,fmt) 1.0
end
""",error='descriptor does not match output item type')
        self.check("""character(8)::text
write(text,'(i1,/,i1)') 1,2
end
""",error='multiple records are not yet supported')

    def test_pure_readonly_nonlocal_and_external_writes(self):
        self.reject("""pure subroutine f(text)
character(*),intent(in)::text
write(text,'(i0)') 1
end subroutine
""",'cannot modify dummy argument `text`')
        self.reject("""module m
character(8)::text
contains
pure subroutine f()
write(text,'(i0)') 1
end subroutine
end module
""",'cannot modify nonlocal variable `text`')
        self.reject("""pure subroutine f()
write(*,'(i0)') 1
end subroutine
""",'external I/O')

    def test_unsupported_controls_remain_errors(self):
        self.reject("""character(8)::text
write(text,'(i0)',advance='no') 1
end
""",'internal WRITE control options')
        self.reject("""character(8)::text
write(text,*) 1
end
""",'list-directed internal WRITE')

if __name__=='__main__':unittest.main()

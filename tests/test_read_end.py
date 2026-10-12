"""Named READ controls and forward END= transfers, interpreter/native O0/O2."""
import unittest
import test_deallocate

class ReadEnd(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_record_count_function(self):
        from test_record_read import ROOT
        self.check((ROOT/'tests/141_read_end.f90').read_text(), "3\n")

    def test_count_records_nested_loops(self):
        self.check("""program main
integer::n,i
n=0
do
 do i=1,2
  read(fmt=*,unit=5,end=10)
  n=n+1
 end do
end do
10 continue
print *,n
end program main
""", "3\n", data="one\n\nlast")

    def test_forward_skip_without_loop(self):
        program="""program main
read(unit=*,fmt=*,end=10)
print *,'record'
10 continue
print *,'done'
end program main
"""
        self.check(program,"done\n")
        self.check(program,"record\ndone\n",data="line\n")

    def test_formatted_read(self):
        self.check("""program main
character(len=4)::line
integer::n
n=0
do
read(unit=5,fmt='(a)',end=20) line
n=n+1
print *,line
end do
20 continue
print *,n
end program main
""","abcd\n1\n",data="abcd\n")

    def test_named_iostat_and_internal_read(self):
        self.check("""program main
integer::ios,n
character(len=2)::text
text='42'
read(fmt=*,unit=text,iostat=ios) n
print *,n,ios
read(fmt=*,unit=*,iostat=ios)
print *,ios<0
end program main
""","42 0\nT\n")

    def test_named_cycle_crossing_region(self):
        self.check("""program main
integer::i,n
n=0
outer: do i=1,3
read(5,*,end=10)
n=n+1
cycle outer
10 continue
n=n+10
exit outer
end do outer
print *,n
end program main
""", "11\n", data="one\n")

    def test_unqualified_cycle_cannot_be_captured(self):
        self.reject("""program main
integer::i
do i=1,3
read(5,*,end=10)
cycle
10 continue
end do
end program main
""", "use named EXIT/CYCLE")

    def test_input_error_is_not_eof(self):
        self.check("""program main
read(unit=23,fmt=*,end=10)
10 continue
print *,'wrong'
end program main
""",error="input error in READ with END=")

    def test_invalid_controls_and_targets(self):
        for statement,message in [
            ('read(unit=5,unit=5,fmt=*)','duplicate READ control'),
            ('read(unit=5,*)','positional READ controls must precede named'),
            ('read(unit=5,fmt=*,end=77)','undefined statement label 77'),
            ('read(unit=5,fmt=*,end=0)','statement label must be between'),
        ]:
            with self.subTest(statement=statement):
                self.reject('program main\n'+statement+'\nend program main',message)
        self.reject('program main\n10 continue\nread(5,*,end=10)\nend program main','forward label')

if __name__ == '__main__':
    unittest.main()

"""List-directed external input in interpreter/native O0/O2 and gfortran."""
import subprocess
import tempfile
from pathlib import Path
import unittest
import test_deallocate
from test_record_read import ROOT, COMPILER, build_commands

class ListRead(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_mixed_records_and_kinds(self):
        self.check("""integer::i,ios
integer(kind=8)::wide
double precision::x
real::y
logical::flag
character(12)::name
read(5,*,iostat=ios)i,wide,x,y,flag,name
print *,ios,i,wide,x,y,flag,'['//trim(name)//']'
end
""", "0 42 -9223372036854775808 125.0 0.25 T [two words]\n",
                   data='42, -9223372036854775808\n1.25d2,.25, .true., "two words"\n')

    def test_repetition_null_slash_and_position(self):
        self.check("""integer::u,a(6),b
open(newunit=u,status='scratch')
write(u,'(a)')'2*3, ,4 / ignored'
write(u,'(a)')'9,999'
write(u,'(a)')'10'
rewind(u)
a=-1
read(u,*)a
read(u,*)b
print *,a,b
backspace(u)
read(u,*)b
print *,b
read(u,*)b
print *,b
end
""", '3 3 -1 4 -1 -1 9\n9\n10\n')

    def test_long_records_and_continued_quoted_input(self):
        self.check("""integer::n,m
character(12)::s
read(5,*)n,s
read(5,*)m
print *,n,'['//trim(s)//']',m
end
""", '42 [ab"cd] 17\n',data=' '*20000+'42,"ab""\ncd" trailing ignored\n17\n')

    def test_multidimensional_section(self):
        self.check("""integer::a(2,3,2),i
a=-1
do i=1,2
read(5,*)a(i,:,2)
end do
print *,a(:,:,1),a(:,:,2)
end
""", '-1 -1 -1 -1 -1 -1 1 4 2 5 3 6\n',data='1 2 3\n4 5 6\n')

    def test_internal_mixed_input_and_character_arrays(self):
        self.check("""character(80)::source
character(4)::words(2)
integer::n,ios,j
real::x
source='17,2.5,one,"two"'
read(source,*,iostat=ios)n,x,words
print *,ios,n,x,words
source='abcd'
j=2
read(source,*)words(j)
print *,words
end
""",'0 17 2.5 one two\none abcd\n')

    def test_complex_scalars_and_arrays(self):
        self.check("""use iso_fortran_env,only:real64
complex(real64)::z,a(2)
integer::ios
read(5,*,iostat=ios)z,a
print *,ios,real(z),aimag(z),real(a),aimag(a)
end
""",'0 1.0 -2.0 3.0 3.0 4.0 4.0\n',data='(1d0,-2d0),2*(3,4)\n')

    def test_multiple_items_default_stdin(self):
        self.check("""integer::i,j
read *,i,j
print *,i,j
end
""",'12 34\n',data='12\n34\n')

    def test_implied_do_negative_and_runtime_stride(self):
        self.check("""integer::a(5),i,step
a=-1
step=-2
read(*,*)(a(i),i=5,1,step)
print *,a
end
""",'3 -1 2 -1 1\n',data='1 2 3\n')

    def test_failed_read_invalidates_previous_value(self):
        self.check("""integer::n,ios
character(4)::source
source='bad'
n=17
read(source,*,iostat=ios)n
print *,n
end
""",error='read of uninitialized variable `n`',reference=False)

    def test_null_does_not_initialize(self):
        self.check("""integer::n
read(5,*)n
print *,n
end
""",error='read of uninitialized variable `n`',data=',\n',reference=False)

    def test_character_implied_do(self):
        self.check("""character(3)::words(3)
integer::i
read(*,*)(words(i),i=3,1,-1)
print *,words
end
""",'cccbbbaaa\n',data='aaa bbb ccc\n')

    def test_runtime_array_and_zero_size_record(self):
        self.check("""integer,allocatable::a(:,:)
integer::n,z(0),m
read(*,*)n
allocate(a(n,2))
read(*,*)a
read(*,*)z
read(*,*)m
print *,a,m
end
""",'1 2 3 4 17\n',data='2\n1 2\n3 4 unused\nskipped record\n17\n')

    def test_end_branch_with_numeric_list(self):
        self.check("""integer::i,j
read(5,*,end=10)i,j
print *,i,j
10 continue
print *,'done'
end
""",'done\n',data='')

    def test_repetition_across_character_lengths(self):
        self.check("""character(2)::a
character(6)::b
read(5,*)a,b
print *,a,trim(b)
end
""",'ababcd\n',data="2*'abcd'\n")

    def test_repetition_type_mismatch(self):
        self.check("""double precision::x
character(8)::text
integer::ios
read(5,*,iostat=ios)x,text
print *,ios>0
end
""",'T\n',data='2*1D2\n')

    def test_conversion_errors_and_record_recovery(self):
        self.check("""integer::u,ios,n
double precision::x
real::y
open(newunit=u,status='scratch')
write(u,'(a)')'1e+'
write(u,'(a)')'bad'
write(u,'(a)')'2147483648'
write(u,'(a)')'42'
rewind(u)
read(u,*,iostat=ios)x
print *,ios>0
read(u,*,iostat=ios)y
print *,ios>0
read(u,*,iostat=ios)n
print *,ios>0
read(u,*)n
print *,n
end
""",'T\nT\nT\n42\n')

    def test_named_file_unit_zero_and_newunit_reuse(self):
        with tempfile.TemporaryDirectory(prefix='rfort-input-file-') as tmp:
            path=(Path(tmp)/'data.txt').as_posix()
            self.check(f"""integer::u,n
character(512)::path
path='{path}'
open(unit=0,file=path,status='replace',action='write')
write(0,'(i0)')42
close(0)
open(newunit=u,file=path,status='old',action='read')
read(u,*)n
print *,n
close(u)
open(newunit=u,file=path,status='old',action='read')
read(u,*)n
print *,n
close(u)
open(newunit=u,file=path,status='old',action='write')
write(u,'(i0)')7
close(u)
open(newunit=u,file=path,status='old',action='read')
read(u,*)n
print *,n
close(u)
end
""",'42\n42\n7\n')

    def test_character_select_case_padding(self):
        self.check("""character(20)::text
integer::i
do i=1,3
read(*,*)text
select case(text)
case('one','two ')
print *,'matched'
case default
print *,'other'
end select
end do
end
""",'matched\nmatched\nother\n',data='one\ntwo\nthree\n')

    def test_zero_size_read_at_eof(self):
        self.check("""integer::a(0),ios
read(*,*,iostat=ios)a
print *,ios<0
end
""",'T\n',data='')

    def test_invalid_negative_unit_does_not_request_newunit(self):
        self.check("""integer::u
u=-1
open(unit=u,file='unused-negative-unit.txt',status='old',action='read')
end
""",error='file units must be in',reference=False)

    def test_character_array_active_extent(self):
        self.check("""character(3),allocatable::words(:)
integer::n,m
allocate(words(4))
deallocate(words)
n=2
allocate(words(n))
read(*,*)words
read(*,*)m
print *,size(words),words,m
end
""",'2 aaabbb 17\n',data='aaa bbb\n17\n')

    def test_character_input_checks_active_bounds(self):
        self.check("""character(3),allocatable::words(:)
integer::n,j
allocate(words(4))
deallocate(words)
n=2
allocate(words(n))
j=3
read(*,*)words(j)
end
""",error='CHARACTER READ index out of bounds',data='aaa\n',reference=False)

    def test_user_write_read(self):
        # Keep the user's temp.txt intact, and test their actual source unchanged.
        source=ROOT/'tests/io/write_read.f90'
        with tempfile.TemporaryDirectory(prefix='rfort-write-read-') as tmp:
            directory=Path(tmp)
            for command in build_commands(source,directory):
                result=subprocess.run(command,cwd=directory,capture_output=True,text=True,timeout=30)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(result.stderr,'')
                self.assertLess(float(result.stdout.strip()),1e-12)

if __name__=='__main__':
    unittest.main(verbosity=2)

"""Active lengths of CHARACTER function results, interpreter/native/reference."""
from pathlib import Path
import unittest
import test_deallocate

REPRO=(Path(__file__).parent/'180_character_allocatable_result.f90').read_text(encoding='utf-8')
MODULE=REPRO.split('program main')[0]

class CharacterAllocatableResults(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_repro(self):
        self.check(REPRO,'dog boy\n')

    def test_runtime_filters_all_none_some_repeated(self):
        self.check(MODULE+'''program main
use character_exclude_mod
character(len=3)::words(3),remove(3)
words=['dog','cat','boy']
remove=['cat','zzz','zzz']
write(*,'(100(a,1x))') words.exclude.remove
remove=words
print *,size(words.exclude.remove)
write(*,'(100(a,1x))') words.exclude.remove
remove='zzz'
write(*,'(100(a,1x))') words.exclude.remove
print *,size(words.exclude.remove)
end program main
''','dog boy\n0\n\ndog cat boy\n3\n')

    def test_assignment_and_forwarded_result(self):
        self.check(MODULE+'''program main
use character_exclude_mod
character(len=5)::out(2)
out=['dog','cat','boy'].exclude.['cat']
print '(a,1x,a)',trim(out(1)),trim(out(2))
call consume(['dog','cat','boy'].exclude.['cat'])
contains
subroutine consume(x)
character(len=*),intent(in)::x(:)
print *,size(x)
write(*,'(100(a,1x))') x
end subroutine consume
end program main
''','dog boy\n2\ndog boy\n')

    def test_empty_input_and_different_lengths(self):
        self.check(MODULE+'''program main
use character_exclude_mod
print *,size([character(len=3)::].exclude.['cat'])
write(*,'(100(a,1x))') ['dog  ','cat  '].exclude.['cat']
write(*,'(100(a,1x))') ['dog','cat'].exclude.[character(len=5)::]
end program main
''','0\ndog\ndog cat\n')

    def test_pack_scalar_mask_and_vector(self):
        module=MODULE.replace('pack(xx,xuse)','pack(xx,.true.)')
        self.check(module+'''program main
use character_exclude_mod
write(*,'(100(a,1x))') ['dog','cat','boy'].exclude.['cat']
end program main
''','dog cat boy\n')
        module=MODULE.replace('pack(xx,xuse)',"pack(xx,xuse,['abc','def','ghi'])")
        self.check(module+'''program main
use character_exclude_mod
write(*,'(100(a,1x))') ['dog','cat','boy'].exclude.['cat']
end program main
''','dog boy ghi\n')

    def test_result_shape_mismatch(self):
        self.check(MODULE+'''program main
use character_exclude_mod
character(len=3)::out(3)
out=['dog','cat','boy'].exclude.['cat']
end program main
''',error='shapes do not conform')

    def test_false_scalar_mask_and_short_vector(self):
        module=MODULE.replace('pack(xx,xuse)','pack(xx,.false.)')
        self.check(module+'''program main
use character_exclude_mod
print *,size(['dog','cat'].exclude.['cat'])
print *,['dog','cat'].exclude.['cat']
end program main
''','0\n\n')
        module=MODULE.replace('pack(xx,xuse)',"pack(xx,xuse,['abc'])")
        self.check(module+'''program main
use character_exclude_mod
write(*,'(100(a,1x))') ['dog','cat','boy'].exclude.['cat']
end program main
''',error='PACK VECTOR has too few elements')

    def test_list_directed_runtime_format_and_nonadvancing(self):
        self.check(MODULE+'''program main
use character_exclude_mod
character(len=4)::words(3)
character(len=30)::fmt
words=['dog ','cat ','boy ']
fmt='(100(a,1x))'
print *,words.exclude.['cat']
write(*,fmt) words.exclude.['cat']
write(*,fmt,advance='no') words.exclude.['cat']
write(*,'(a)') 'end'
end program main
''','dog boy\ndog boy\ndog boy end\n')

if __name__=='__main__':unittest.main()

"""Array-result and captured internal-WRITE regressions from util.f90."""
import unittest
from test_record_read import ROOT
import test_deallocate


class UtilRegressions(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_calendar_vector_internal_write(self):
        self.check((ROOT/'tests/programs/xnow_format.f90').read_text(),
                   '[2026-10-16 09:08:07]\n')

    def test_partition_array_result(self):
        self.check((ROOT/'tests/programs/xpartition_function.f90').read_text(),
                   '3 5 8\n1 1 1\n1 2 3 4\n0\n')

    def test_internal_write_array_expressions(self):
        self.check('''integer::a(2,3),indices(3)
character(40)::text
a=reshape([1,2,3,4,5,6],[2,3])
indices=[3,1,3]
write(text,"(6i3)") a(:,indices)
print *,trim(text)
write(text,"(6i3)") a+1
print *,trim(text)
write(text,"(6i3)") a(:,2:1:-1)
print *,trim(text)
end
''','5 6 1 2 5 6\n2 3 4 5 6 7\n3 4 1 2\n')

    def test_internal_write_character_and_logical(self):
        self.check('''character(2)::words(3)
logical::flags(3)
character(30)::text
words=['aa','bb','cc']
flags=[.true.,.false.,.true.]
write(text,"(3a2)") words([3,1,3])
print *,trim(text)
write(text,"(3l2)") flags([2,3,1])
print *,trim(text)
end
''','ccaacc\nF T T\n')

    def test_internal_write_empty_and_runtime_selection(self):
        self.check('''integer,allocatable::a(:),indices(:)
character(40)::text
allocate(a(4),indices(3))
a=[10,20,30,40]
indices=[4,2,4]
write(text,"(3i3)") a(indices)
print *,trim(text)
write(text,"(i3)") a(2:1)
print *,len_trim(text)
end
''','40 20 40\n0\n')


if __name__ == '__main__':
    unittest.main(verbosity=2)

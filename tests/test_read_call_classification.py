"""User procedure names containing 'read' must not become READ operations."""
import unittest

import test_deallocate
from test_record_read import ROOT


class ReadCallClassification(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    @staticmethod
    def source():
        return (ROOT/'tests/programs/xacv_read_named_module_section.f90').read_text()

    def test_xacv_reproducer(self):
        self.check(self.source(), '1 2 3\n')

    def test_names_containing_read(self):
        for module, procedure in [('copy_mod', 'read_copy_array'),
                                  ('bread_mod', 'copy_array'),
                                  ('copy_mod', 'spread_values')]:
            with self.subTest(module=module, procedure=procedure):
                self.check(self.source().replace('read_copy_mod', module)
                           .replace('copy_array', procedure), '1 2 3\n')

    def test_fixed_vector_actual(self):
        self.check(self.source().replace('integer,allocatable::a(:),b(:)',
                                        'integer::a(3),b(3)')
                   .replace('allocate(a(n),b(n))', '')
                   .replace('a(:n),b', 'a([3,1,2]),b'), '3 1 2\n')

    def test_runtime_vector_actual(self):
        self.check(self.source().replace('integer::n', 'integer::n\ninteger,allocatable::idx(:)')
                   .replace('allocate(a(n),b(n))', 'allocate(a(n),b(n),idx(n))\nidx=[3,1,2]')
                   .replace('a(:n),b', 'a(idx),b'), '3 1 2\n')

    def test_writable_triplet_actual(self):
        self.check(self.source().replace('allocate(a(n),b(n))',
                                        'allocate(a(n),b(n+1))\nb=-1')
                   .replace('a(:n),b', 'a(:n),b(2:n+1)'), '-1 1 2 3\n')

    def test_real_reads_still_reject_unsupported_vector_destinations(self):
        for statement in ('read *,a([3,1])',
                          "read(10,*) a([3,1])",
                          "read(10,'(2i2)',iostat=ios) a([3,1])",
                          "read(10) a([3,1])"):
            with self.subTest(statement=statement):
                self.reject('implicit none\ninteger::a(3),ios\n'+statement+'\nend\n',
                            'READ into vector-subscript selections is not yet supported')


if __name__ == '__main__':
    unittest.main()

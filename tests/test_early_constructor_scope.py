"""Constructor index visibility during early saved-object constant folding."""
import unittest
import test_deallocate
from test_record_read import ROOT


FIXTURE = (ROOT / 'tests/programs/xfve_constructor_implied_do.f90').read_text()


def nested_block(declaration=''):
    return FIXTURE.replace('do ilambda=1,1\n', 'do ilambda=1,1\nblock\n' + declaration).replace(
        '   end do\nend do loop_ifwd', 'end block\n   end do\nend do loop_ifwd')


class EarlyConstructorScope(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_fve_reproducer(self):
        self.check(FIXTURE, '2.0 3.0 4.0\n')

    def test_block_uses_enclosing_integer_without_changing_it(self):
        program = nested_block().replace('loop_ifwd: do', 'imetric = 99\nloop_ifwd: do').replace(
            "print '(3f6.1)', metrics_mean(1,:)", "print '(3f6.1)', metrics_mean(1,:)\nprint *, imetric")
        self.check(program, '2.0 3.0 4.0\n99\n')

    def test_local_integer_hides_enclosing_real(self):
        program = nested_block('integer :: imetric\n').replace(
            'integer :: ifwd, ilambda, imetric', 'integer :: ifwd, ilambda\nreal :: imetric')
        self.check(program, '2.0 3.0 4.0\n')

    def test_module_integer(self):
        program = FIXTURE.replace('integer, parameter :: nmetrics = 3',
                                  'integer, parameter :: nmetrics = 3\ninteger :: imetric').replace(
            'integer :: ifwd, ilambda, imetric', 'integer :: ifwd, ilambda')
        self.check(program, '2.0 3.0 4.0\n')

    def test_runtime_bounds(self):
        program = FIXTURE.replace('mean(metrics(:,imetric))', 'real(imetric)').replace(
            'imetric=1,nmetrics', 'imetric=1,size(metrics,2)')
        self.check(program, '1.0 2.0 3.0\n')

    def test_invalid_shadowed_or_missing_indices(self):
        for declaration in ['real :: imetric\n', 'integer :: imetric(2)\n',
                            'integer, parameter :: imetric=1\n']:
            with self.subTest(declaration=declaration):
                self.reject(nested_block(declaration), 'locally declared scalar INTEGER')
        self.reject(FIXTURE.replace('integer :: ifwd, ilambda, imetric',
                                    'integer :: ifwd, ilambda'), 'locally declared scalar INTEGER')

    def test_constructor_does_not_initialize_enclosing_variable(self):
        self.reject(nested_block().replace("print '(3f6.1)', metrics_mean(1,:)",
                                          'print *, imetric'), 'uninitialized variable `imetric`')


if __name__ == '__main__':
    unittest.main(verbosity=2)

"""Array descriptors must respect local declarations across specialized calls."""
import unittest

import test_deallocate
from test_record_read import ROOT


REPRO = (ROOT/'tests/programs/xacv_scalar_matrix_name_collision.f90').read_text()


class ArrayScopeMetadata(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_xacv_repro(self):
        self.check(REPRO, '1.2500\n')

    def test_caller_matrix_types_and_ranks(self):
        for kind, value in [('integer', '7'), ('real', '2.0'),
                            ('logical', '.true.'), ('character(3)', "'abc'")]:
            with self.subTest(kind=kind):
                program = REPRO.replace('double precision, allocatable :: xx(:,:)',
                                        f'{kind}, allocatable :: xx(:,:,:)')
                program = program.replace('allocate(xx(2,2))', 'allocate(xx(2,1,2))')
                program = program.replace('xx = 1d0', f'xx = {value}')
                self.check(program, '1.2500\n')

    def test_local_fixed_array_shadows_caller_descriptor(self):
        program = REPRO.replace('double precision :: xx\nxx = x*x\nanswer = xx + 1d0',
                                'double precision :: xx(2)\nxx = [x,x+1d0]\nanswer = sum(xx)')
        self.check(program, '2.0000\n')

    def test_scalar_dummy_shadows_caller_descriptor(self):
        program = REPRO.replace('subroutine evaluate(x, answer)',
                                'subroutine evaluate(xx, answer)')
        program = program.replace('double precision, intent(in) :: x\ndouble precision, intent(out)',
                                  'double precision, intent(in) :: xx\ndouble precision, intent(out)')
        program = program.replace('double precision :: xx\nxx = x*x\nanswer = xx + 1d0',
                                  'answer = xx*xx + 1d0')
        self.check(program, '1.2500\n')

    def test_pointer_metadata_does_not_attach_to_scalar(self):
        program = REPRO.replace('double precision, allocatable :: xx(:,:)',
                                'double precision, pointer :: xx(:,:)\n'
                                'double precision, target :: storage(2,2)')
        program = program.replace('allocate(xx(2,2))', 'xx => storage')
        program = program.replace('deallocate(xx)', 'nullify(xx)')
        self.check(program, '1.2500\n')

    def test_assumed_shape_association_retains_its_own_extents(self):
        self.check('''program main
implicit none
double precision,allocatable::xx(:,:),other(:,:)
double precision::answer
allocate(xx(2,2),other(3,1))
xx=100d0
other=2d0
call total(other,answer)
print *,answer
deallocate(xx,other)
contains
pure subroutine total(xx,answer)
double precision,intent(in)::xx(:,:)
double precision,intent(out)::answer
answer=sum(xx)
end subroutine
end program
''', '6.0\n')

    def test_uninitialized_scalar_is_still_rejected(self):
        self.reject(REPRO.replace('xx = x*x\n', ''), 'uninitialized variable')


if __name__ == '__main__':
    unittest.main()

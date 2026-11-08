"""Nested scalar records reuse the complete component-array regression matrix."""
import unittest
import re
from test_record_read import ROOT
import test_concurrent_parallel_record_arrays


class ParallelNestedRecords(test_concurrent_parallel_record_arrays.ParallelRecordArrays):
    @staticmethod
    def example():
        # Apply the parent tests to leaf operations, then nest the record at the
        # last step so each test still exercises its original transformation.
        return (ROOT/'tests/programs/xconcurrent_parallel_record_arrays.f90').read_text()

    @staticmethod
    def nested(source):
        for name in ('particle', 'entry'):
            source = source.replace(f'type :: {name}\n', f'type :: {name}_leaf\n', 1)
            if f'type :: {name}_leaf' in source:
                source = source.replace('end type',
                    f'end type\ntype :: {name}\n type({name}_leaf) :: state\nend type', 1)
        source = re.sub(r'%(position|velocity|values|flags|matrix|v)\b', r'%state%\1', source)
        return source

    def check_source(self, source, **kwargs):
        return super().check_source(self.nested(source), **kwargs)

    def build_run(self, source, expected, **kwargs):
        return super().build_run(self.nested(source), expected, **kwargs)

    def test_scalar_leaves_and_sibling_fields(self):
        source = """implicit none
type :: state_type
 integer :: value
 logical :: active
end type
type :: particle
 type(state_type) :: left, right
 real :: scale
end type
type(particle) :: a(9000)
integer :: i
do i=1,9000
 a(i)%left%value=i
 a(i)%left%active=.true.
 a(i)%right%value=2*i
 a(i)%right%active=.false.
 a(i)%scale=2.0
end do
do concurrent(i=1:9000,a(i)%left%active)
 a(i)%left%value=a(i)%left%value+a(i)%right%value
 a(i)%right%active=.not.a(i)%right%active
 a(i)%scale=a(i)%scale+1.0
end do
do i=1,9000
 if(a(i)%left%value/=3*i) stop 1
 if(.not.a(i)%right%active) stop 2
 if(a(i)%scale/=3.0) stop 3
end do
print *, 'PASS'
end
"""
        super().check_source(source)

    def test_multiple_nested_levels(self):
        source = (ROOT/'tests/programs/xconcurrent_parallel_nested_records.f90').read_text()
        source = source.replace('type :: particle',
            'type :: wrapper\n type(state_type) :: inner\nend type\ntype :: particle')
        source = source.replace('type(state_type) :: state', 'type(wrapper) :: state')
        source = source.replace('%state%', '%state%inner%')
        super().check_source(source)

    def test_unsafe_nested_sibling_keeps_serial(self):
        source = (ROOT/'tests/programs/xconcurrent_parallel_nested_records.f90').read_text()
        source = source.replace('real :: position(3)', 'real,allocatable :: scratch(:)\n   real :: position(3)')
        super().check_source(source, reason='serial fallback:')

    def test_standalone_example(self):
        # Bypass the transformation: this is the user-facing source on disk.
        super().check_source((ROOT/'tests/programs/xconcurrent_parallel_nested_records.f90').read_text())


if __name__ == '__main__':
    unittest.main()

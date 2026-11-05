"""Mixed intrinsic and defined input in interpreter/native modes."""
import unittest
from pathlib import Path
import test_deallocate
import test_defined_input_formats as formats
import test_defined_input_arrays as arrays

class DefinedInputMixed(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((Path(__file__).parent / 'programs/xdefined_formatted_input_mixed.f90').read_text(),
                   '10 1 3 2 4 5.5\n')

    def test_list_complex(self):
        self.check(arrays.DefinedInputArrays().source(
            'complex :: z\ntype(point) :: a',
            'read(u,*) z,a\nprint *,real(z),aimag(z),a%x,a%y',
            '(3,4) 1 2'),'3 4 1 2\n')

    def test_mixed_sections(self):
        self.check(formats.DefinedInputFormats().source(
            'integer :: n(3)\ntype(point) :: a(2)\nreal :: r',
            'n=-1\nread(u,"(2i3,2dt,f5.1)") n(3:1:-2),a(2:1:-1),r\nprint *,n,a%x,a%y,r',
            [' 10 20  1  2  3  4  5.5']),
            '20 -1 10 3 1 4 2 5.5\n')

    def test_empty_intrinsic_section(self):
        self.check(formats.DefinedInputFormats().source(
            'integer :: n(2)\ntype(point) :: a',
            'n=-1\nread(u,"(dt)") n(2:1),a\nprint *,n,a%x,a%y',
            ['  1  2']),'-1 -1 1 2\n')

    def test_explicit_complex_boundary(self):
        self.reject(formats.DefinedInputFormats().source(
            'complex :: z\ntype(point) :: a',
            'read(u,"(2f5.1,dt)") z,a'),
            'this intrinsic type is not yet supported in mixed formatted input')

    def test_list_scalar(self):
        self.check(arrays.DefinedInputArrays().source(
            'integer :: n\ntype(point) :: a\nreal :: r',
            'read(u,*) n,a,r\nprint *,n,a%x,a%y,r','10 1 2 3.5'),'10 1 2 3.5\n')

    def test_explicit_scalar(self):
        self.check(formats.DefinedInputFormats().source(
            'integer :: n\ntype(point) :: a\nreal :: r',
            'read(u,"(i3,dt,f5.1)") n,a,r\nprint *,n,a%x,a%y,r',
            [' 10  1  2  3.5']),'10 1 2 3.5\n')

    def test_list_arrays(self):
        self.check(arrays.DefinedInputArrays().source(
            'integer :: n(2)\ntype(point) :: a(2)\nreal :: r(2)',
            'read(u,*) n,a,r\nprint *,n,a%x,a%y,r','10 20 1 2 3 4 5.5 6.5'),
            '10 20 1 3 2 4 5.5 6.5\n')

    def test_explicit_arrays(self):
        self.check(formats.DefinedInputFormats().source(
            'integer :: n(2)\ntype(point) :: a(2)\nreal :: r(2)',
            'read(u,"(2i3,2dt,2f5.1)") n,a,r\nprint *,n,a%x,a%y,r',
            [' 10 20  1  2  3  4  5.5  6.5']),
            '10 20 1 3 2 4 5.5 6.5\n')

    def test_character_logical(self):
        self.check(formats.DefinedInputFormats().source(
            'character(3) :: s\nlogical :: flag\ntype(point) :: a',
            'read(u,"(a3,dt,l1)") s,a,flag\nprint *,s,a%x,a%y,flag',
            ['abc  1  2T']),'abc 1 2 T\n')

    def test_list_character_logical(self):
        self.check(arrays.DefinedInputArrays().source(
            'character(3) :: s\nlogical :: flag\ntype(point) :: a',
            'read(u,*) s,a,flag\nprint *,s,a%x,a%y,flag',
            'abc 1 2 T'),'abc 1 2 T\n')

    def test_intrinsic_failure_stops_derived(self):
        self.check(arrays.DefinedInputArrays().source(
            'integer :: n\ntype(point) :: a',
            'a%x=-1\nread(u,*,iostat=status,iomsg=message) n,a\nprint *,status>0,a%x,len_trim(message)>0',
            'bad 1 2'),'T -1 T\n')

    def test_reader_failure_stops_intrinsic(self):
        source=arrays.DefinedInputArrays().source(
            'integer :: n\ntype(point) :: a',
            'n=-1\nread(u,*,iostat=status,iomsg=message) a,n\nprint *,status>0,n,len_trim(message)>0',
            'bad 1 2')
        self.check(source,'T -1 T\n',reference=False)

    def test_descriptor_mismatch(self):
        self.check(formats.DefinedInputFormats().source(
            'integer :: n\ntype(point) :: a',
            'n=-1\na%x=-1\nread(u,"(dt,i3)",iostat=status,iomsg=message) n,a\nprint *,status>0,n,a%x,len_trim(message)>0'),
            'T -1 -1 T\n',reference=False)

    def test_spacing_and_reversion(self):
        self.check(formats.DefinedInputFormats().source(
            'integer :: n,m\ntype(point) :: a,b',
            'read(u,"(i3,2x,dt)") n,a,m,b\nprint *,n,a%x,m,b%x',
            [' 10xx  1  2',' 20xx  3  4']),'10 1 20 3\n')

if __name__=='__main__':
    unittest.main()

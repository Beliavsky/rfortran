"""Literal parent formats for defined input, with a runtime format cursor."""
import unittest
import test_defined_input_arrays as arrays
import test_deallocate

class DefinedInputFormats(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def source(self, declaration, body, records=None):
        source=arrays.DefinedInputArrays().source(declaration,body,
            '  1  2  3  4  5  6  7  8  9 10 11 12')
        a=source.index('read(unit,');b=source.index('end subroutine',a)
        source=source[:a]+"read(unit,'(2i3)',iostat=iostat,iomsg=iomsg) dtv%x,dtv%y\n"+source[b:]
        if records is not None:
            a=source.index("write(u,'(a)')");b=source.index('rewind(u)',a)
            source=source[:a]+''.join("write(u,'(a)') '"+r+"'\n" for r in records)+source[b:]
        return source

    def test_repeat(self):
        self.check(self.source('type(point) :: a(3)',
            'read(u,"(3dt)") a\nprint *,a%x\nprint *,a%y'),'1 3 5\n2 4 6\n')

    def test_reversion(self):
        self.check(self.source('type(point) :: a(3)',
            'read(u,"(dt)") a\nprint *,a%x\nprint *,a%y',
            ['  1  2 ignored','  3  4 ignored','  5  6 ignored']),'1 3 5\n2 4 6\n')

    def test_nested_repeat(self):
        self.check(self.source('type(point) :: a(2,3)',
            'read(u,"(3(2dt))") a\nprint *,a%x'),'1 3 5 7 9 11\n')

    def test_array_then_scalar(self):
        self.check(self.source('type(point) :: a(2),b',
            'read(u,"(3dt)") a,b\nprint *,a%x,b%x,b%y'),'1 3 5 6\n')

    def test_empty_array(self):
        self.check(self.source('type(point) :: a(0),b',
            'read(u,"(dt)") a,b\nprint *,b%x,b%y'),'1 2\n')

    def test_unlimited(self):
        self.check(self.source('type(point) :: a(3)',
            'read(u,"(*(dt))") a\nprint *,a%x'),'1 3 5\n')

    def test_metadata(self):
        source=self.source('type(point) :: a(3)',
            'read(u,"(dt\'one\'(1),dt\'two\'(2,3),dt)") a')
        source=source.replace("read(unit,'(2i3)'","print *,trim(iotype),size(v_list),v_list\nread(unit,'(2i3)'")
        self.check(source,'DTone 1 1\nDTtwo 2 2 3\nDT 0\n')

    def test_slash(self):
        self.check(self.source('type(point) :: a(2)',
            'read(u,"(dt,/,dt)") a\nprint *,a%x',
            ['  1  2 ignored','  3  4 ignored']),'1 3\n')

    def test_spacing(self):
        self.check(self.source('type(point) :: a(2)',
            'read(u,"(2x,dt,3x,dt)") a\nprint *,a%x',
            ['xx  1  2xxx  3  4']),'1 3\n')

    def test_trailing_slash(self):
        self.check(self.source('type(point) :: a,b',
            'read(u,"(dt,/)") a\nread(u,"(dt)") b\nprint *,a%x,b%x',
            ['  1  2','skip this','  3  4']),'1 3\n')

    def test_colon_suppresses_trailing_slash(self):
        self.check(self.source('type(point) :: a,b',
            'read(u,"(dt,:,/)") a\nread(u,"(dt)") b\nprint *,a%x,b%x',
            ['  1  2','  3  4']),'1 3\n')

    def test_nested_reversion(self):
        self.check(self.source('type(point) :: a(5)',
            'read(u,"(dt,2(dt))") a\nprint *,a%x',
            ['  1  2  3  4  5  6','  7  8  9 10']),'1 3 5 7 9\n')

    def test_runtime_matrix_section(self):
        source=self.source('type(point),allocatable :: a(:,:)',
            'allocate(a(2,2))\ncall fill(u,a)\nprint *,a%x',
            ['  1  2  3  4','  5  6  7  8']).replace('close(u)\nend program',"""close(u)
contains
subroutine fill(unit,values)
integer,intent(in) :: unit
type(point),intent(out) :: values(:,:)
read(unit,'(2dt)') values(:,2:1:-1)
end subroutine
end program""")
        self.check(source,'5 7 1 3\n')

    def test_vector_section(self):
        self.check(self.source('type(point) :: a(3)',
            'a%x=-1\nread(u,"(2dt)") a([3,1])\nprint *,a%x'),'3 -1 1\n')

    def test_eof_on_reversion(self):
        self.check(self.source('type(point) :: a(3)',
            'a%x=-1\nread(u,"(dt)",iostat=status,iomsg=message) a\nprint *,status<0,a(1)%x,a(3)%x,len_trim(message)>0',
            ['  1  2']),'T 1 -1 T\n',reference=False)

    def test_backward_positioning(self):
        self.check(self.source('type(point) :: a(2)',
            'read(u,"(dt,t1,dt)") a\nprint *,a%x,a%y'),'1 1 2 2\n')

    def test_error_stops(self):
        self.check(self.source('type(point) :: a(3)',
            'a%x=-1\nread(u,"(3dt)",iostat=status,iomsg=message) a\nprint *,status>0,a(1)%x,a(3)%x,len_trim(message)>0',
            ['  1  2bad  4  5  6']),'T 1 -1 T\n',reference=False)

    def test_runtime_format(self):
        self.check(self.source('type(point) :: a(2)\ncharacter(10) :: fmt',
            'fmt="(2dt)"\nread(u,fmt) a'),'')

if __name__=='__main__':
    unittest.main()

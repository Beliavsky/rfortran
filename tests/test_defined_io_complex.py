"""Two real edit descriptors per COMPLEX item in mixed defined I/O lists."""
from pathlib import Path
import unittest
import test_deallocate

SOURCE = (Path(__file__).parent / 'programs/xdefined_io_complex.f90').read_text()


class ComplexDefinedIO(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def source(self, declarations, body):
        return SOURCE.split('program main')[0] + '''program main
use complex_mixed_io
implicit none
integer,parameter :: dp=kind(0d0)
type(point) :: p
integer :: u,ios
character(100) :: msg,fmt,buffer
''' + declarations + '\np%x=7\n' + body + '\nend program\n'

    def test_runtime_internal_roundtrip(self):
        self.check(SOURCE, '1.25 -2.50 7 3.50 4.75\n1.25 -2.50 3.50 4.75 7\n')

    def test_literal_internal_roundtrip(self):
        self.check(SOURCE.replace('write(buffer,fmt)', "write(buffer,'(2f8.2,dt,2f8.2)')").replace('read(buffer,fmt)', "read(buffer,'(2f8.2,dt,2f8.2)')"),
                   '1.25 -2.50 7 3.50 4.75\n1.25 -2.50 3.50 4.75 7\n')

    def test_external_arrays_sections(self):
        self.check(self.source('complex(dp) :: z(2),w(2)', '''
z=[cmplx(1d0,2d0,dp),cmplx(3d0,4d0,dp)]
w=0
open(newunit=u,status='scratch',form='formatted')
fmt='(4f5.1,dt)'
write(u,fmt) z(2:1:-1),p
rewind(u)
read(u,fmt) w,p
close(u)
print *,real(w),aimag(w),p%x
'''), '3 1 4 2 7\n')

    def test_input_vector_section(self):
        self.check(self.source('complex :: z(3)', '''
z=0
buffer='  1.0  2.0  3.0  4.0  7'
read(buffer,'(4f5.1,dt)') z([3,1]),p
print *,real(z),aimag(z),p%x
'''), '3 0 1 4 0 2 7\n', reference=False)  # gfortran -fcheck=all warns about a temporary.

    def test_reversion_between_parts(self):
        self.check(self.source('complex :: z\ncharacter(8) :: records(2)', '''
z=(1.0,2.0)
fmt='(dt,(f5.1))'
write(records,fmt) p,z
print '(a)',trim(records(1)),trim(records(2))
z=0
read(records,fmt) p,z
print *,real(z),aimag(z),p%x
'''), '7 1.0\n2.0\n1 2 7\n')

    def test_slash_between_parts(self):
        self.check(self.source('complex :: z\ncharacter(8) :: records(2)', '''
z=(1.0,2.0)
write(records,'(f5.1,/,f5.1,dt)') z,p
z=0
read(records,'(f5.1,/,f5.1,dt)') z,p
print *,real(z),aimag(z),p%x
'''), '1 2 7\n')

    def test_empty_complex_section(self):
        self.check(self.source('complex :: z(2)', '''
z=0
write(buffer,'(dt)') z(2:1),p
read(buffer,'(dt)') z(2:1),p
print *,p%x
'''), '7\n')

    def test_output_expression_evaluated_once(self):
        self.check(self.source('integer :: calls', '''
calls=0
write(buffer,'(2f5.1,dt)') value(),p
print '(a)',trim(buffer)
print *,calls
contains
function value() result(z)
complex :: z
calls=calls+1
z=(1.0,2.0)
end function
'''), '1.0 2.0 7\n1\n')

    def test_input_second_part_error(self):
        self.check(self.source('complex :: z', '''
buffer='  1.0  bad  9'
msg='unchanged'
read(buffer,'(2f5.1,dt)',iostat=ios,iomsg=msg) z,p
print *,ios/=0,len_trim(msg)>0,p%x
'''), 'T T 7\n')

    def test_output_descriptor_mismatch(self):
        self.check(self.source('complex :: z', '''
z=(1.0,2.0)
msg='unchanged'
fmt='(f5.1,i3,dt)'
write(buffer,fmt,iostat=ios,iomsg=msg) z,p
print *,ios/=0,len_trim(msg)>0
'''), 'T T\n')

    def test_input_dt_between_parts(self):
        self.check(self.source('complex :: z', '''
buffer='  1.0  9'
fmt='(f5.1,dt,f5.1)'
read(buffer,fmt,iostat=ios,iomsg=msg) z,p
print *,ios/=0,p%x
'''), 'T 7\n')

    def test_double_precision_preserved(self):
        self.check(self.source('complex(dp) :: z,w', '''
z=cmplx(1.23456789012345_dp,-9.87654321098765_dp,dp)
write(buffer,'(dt,2f24.15)') p,z
read(buffer,'(dt,2f24.15)') p,w
print *,abs(real(w)-real(z))<1d-14,abs(aimag(w)-aimag(z))<1d-14
'''), 'T T\n')

    def test_kind_keyword(self):
        self.check(self.source('complex(kind=dp) :: z', '''
buffer='  1.0  2.0  7'
read(buffer,'(2f5.1,dt)') z,p
write(*,'(dt,2f5.1)') p,z
'''), '7 1.0 2.0\n')


if __name__ == '__main__':
    unittest.main()

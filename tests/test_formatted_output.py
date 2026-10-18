"""Edit descriptors, format reversion, list-directed file and internal WRITE,
CLOSE with STATUS, SIGN, DIM, DPROD, array MERGE and REAL/INTEGER MATMUL.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_formatted_output.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class FormattedOutput(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_real_descriptors(self):
        self.check("""program p
implicit none
real :: x = 3.14159
double precision :: d = 2.718281828d0
print '(e12.4,"|",es12.4,"|",en12.4,"|",g12.4,"|",f10.3)', x, x, x, x, x
print '(e12.4,"|",es12.4,"|",en12.4,"|",g12.4)', -2.5e-7, -2.5e-7, 123456.7, 1.0e10
print '(e15.6e3,"|",d12.4,"|",1p,e12.4,"|",-1p,e12.4,"|",2p,f10.3)', d, d, d, d, d
print '(g0,"|",g0,"|",g0.3,"|",g0.3)', x, d, x, 0.0
print '(f0.3,"|",f0.0,"|",f4.3,"|",f3.3,"|",e9.3,"|",e8.3)', 0.5, 0.4, 0.5, 0.5, 0.5, 0.5
print '(g10.3,"|",g10.3,"|",g12.4e3)', 0.0999999, 99.97, 3.0
end program
""", "0.3142E+01| 3.1416E+00| 3.1416E+00| 3.142 | 3.142\n"
     "-0.2500E-06| -2.5000E-07|123.4567E+03| 0.1000E+11\n"
     "0.271828E+001| 0.2718D+01| 2.7183E+00| 0.0272E+02| 271.828\n"
     "3.14159012|2.7182818279999998|3.14|0.00\n"
     ".500|0.|.500|***|0.500E+00|.500E+00\n"
     "0.100 | 100. | 3.000 \n")

    def test_integer_logical_and_character_descriptors(self):
        self.check("""program p
implicit none
integer :: i = 42
print '(b8.8,1x,b0,1x,o6,1x,o0,1x,z4,1x,z0,1x,z8.6)', i, 5, i, 8, 255, 255, 255
print '(z0,1x,b0)', -1, 0
print '(sp,i5,f7.2,e11.3,ss,i5,sp,g0)', i, 1.5, 2.5, 7, 3
print '(i5.3,1x,i0,"|",i3.0,"|")', -7, 0, 0
print '(g0,1x,g0,1x,g0,1x,g5.1,"|",l3)', i, .true., 'abc', 7, .false.
end program
""", "00101010 101 52 10 FF FF 0000FF\nFFFFFFFF 0\n+42 +1.50 +0.250E+01 7+3\n"
     "-007 0| |\n42 T abc 7| F\n")

    def test_positioning(self):
        self.check("""program p
implicit none
print '(t5,a,t2,a)', 'xy', 'z'
print '(a,tr3,a,tl2,a)', 'ab', 'cd', 'e'
print '(3x,a,2x,"|")', 'q'
print '(i3/i3//i3)', 1, 2, 3
end program
""", " z  xy\nab   ed\n   q  |\n  1\n  2\n\n  3\n")

    def test_format_reversion(self):
        self.check("""program p
implicit none
integer :: v(3) = [1, 2, 3]
print '(i3)', v
print '(a)', 'a', 'b'
print '("h",2(i2,"-"),"t")', 1, 2, 3, 4, 5
print '(i2,(i3,"|"))', 1, 2, 3, 4
print '(2(i2),3(i3,"|"))', 1, 2, 3, 4, 5, 6, 7, 8
print '(i4,:," next")', 1
print '(2(i3,:,","))', v
end program
""", "1\n2\n3\na\nb\nh 1- 2-t\n 3- 4-t\n 5-\n 1  2|\n  3|\n  4|\n"
     " 1 2  3|  4|  5|\n  6|  7|  8|\n   1\n  1,  2,\n  3\n")

    def test_list_directed_file_and_internal_write(self):
        self.check("""program p
implicit none
integer :: i = 42, u, j
real :: x = 2.5, y
character(len=40) :: s
character(len=80) :: line
write(s,*) 'n =', i
print '(a)', trim(adjustl(s))
open(newunit=u, file='rfort_list_write.txt')
write(u,*) i, x, .true., 'w'
close(u)
open(newunit=u, file='rfort_list_write.txt')
read(u,*) j, y
print *, j, y
rewind(u)
read(u,'(a)') line
print '(a)', trim(adjustl(line))
close(u, status='delete')
open(10, file='rfort_list_write.txt', status='replace')
write(10,'(i3)') [4, 5, 6]
close(10)
open(10, file='rfort_list_write.txt', status='old')
read(10,*) j
read(10,*) j
print *, j
close(10, status='delete')
end program
""", "n = 42\n42 2.5\n42 2.5 T w\n5\n")

    def test_sign_dim_dprod_merge_and_matmul(self):
        self.check("""program p
implicit none
real :: v(3) = [1.0, 0.0, -1.0], m(2,2), c(2,2), w(2) = [1.0, 2.0]
integer :: im(2,2), iv(2) = [1, 1]
logical :: l(3) = [.true., .false., .true.]
m = reshape([1.0, 2.0, 3.0, 4.0], [2, 2])
im = reshape([1, 2, 3, 4], [2, 2])
print *, sign(3.0, -1.0), sign(-3, 2), sign(1.0, -0.0), dim(5, 3), dim(2.0, 5.0), dprod(2.0, 3.0)
print *, sign(-12.0, v), dim(v, 0.5)
print *, merge(v, -v, l), merge(1, 2, v(1) > 0)
print *, sum(merge(v, 0.0, l))
print *, matmul(m, w), matmul(im, iv), matmul(w, m)
c = matmul(m, im)
print *, c
print '(2f6.1)', matmul(transpose(m), m)
end program
""", "-3.0 3 -1.0 2 0.0 6.0\n12.0 12.0 -12.0 0.5 0.0 0.0\n1.0 -0.0 -1.0 1\n0.0\n"
     "7.0 10.0 4 6 5.0 11.0\n7.0 10.0 15.0 22.0\n5.0 11.0\n11.0 25.0\n")


if __name__ == "__main__":
    unittest.main()

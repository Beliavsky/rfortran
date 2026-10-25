"""CHARACTER shifts and UNPACK preserve lengths, padding and array order."""
import unittest
import subprocess
from test_record_read import ROOT, COMPILER
from run_ofort_compat import equivalent
import test_deallocate as helpers

class CharacterTransforms(unittest.TestCase):
    check=helpers.Deallocate.check

    def test_fixed_and_overlap(self):
        self.check("""implicit none
character(3)::a(3),b(3)
a=['ab ','c  ','def']
b=cshift(a,1)
print '(3(a,a,a))','[',b(1),']','[',b(2),']','[',b(3),']'
a=eoshift(a,-1)
print '(3(a,a,a))','[',a(1),']','[',a(2),']','[',a(3),']'
b=unpack(['xx ','yy '],[.true.,.false.,.true.],'---')
print '(3(a,a,a))','[',b(1),']','[',b(2),']','[',b(3),']'
end
""", '[c  ][def][ab ]\n[   ][ab ][c  ]\n[xx ][---][yy ]\n')

    def test_runtime_matrix_and_arguments(self):
        self.check("""implicit none
integer::n
character(2),allocatable::a(:,:),b(:,:)
n=2
allocate(a(n,3))
a=reshape(['a ','b ','c ','d ','e ','f '],[2,3])
call show(cshift(a,[1,-1],dim=2))
b=eoshift(a,[1,-1],['x ','y '],dim=2)
call show(b)
b=unpack(['u ','v ','w '],a=='a '.or.a=='d '.or.a=='e ',a)
call show(b)
contains
subroutine show(x)
character(*),intent(in)::x(:,:)
print *,shape(x),len(x)
print '(6(a,a,a))','[',x(1,1),']','[',x(2,1),']','[',x(1,2),']','[',x(2,2),']','[',x(1,3),']','[',x(2,3),']'
end subroutine
end
""", '2 3 2\n[c ][f ][e ][b ][a ][d ]\n2 3 2\n[c ][y ][e ][b ][x ][d ]\n2 3 2\n[u ][b ][c ][v ][w ][f ]\n')

    def test_deferred_length_and_reallocation(self):
        self.check((ROOT/"tests/programs/xcharacter_transforms_deferred.f90").read_text(), '3 3\n[two][six][one]\n3 3\n[   ][one][two]\n2 2\n[cd][ab]\n', interpreter=False)

    def test_deferred_interpreter(self):
        result=subprocess.run([COMPILER,"run",ROOT/"tests/programs/xcharacter_transforms_deferred.f90"],capture_output=True,text=True,timeout=90)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertTrue(equivalent(result.stdout,'3 3\n[two][six][one]\n3 3\n[   ][one][two]\n2 2\n[cd][ab]\n'),result.stdout)

    def test_deferred_unpack_and_boundary(self):
        self.check("""implicit none
integer::n
character(:),allocatable::a(:),b(:)
n=2
allocate(character(len=n)::a(2))
a=['ab','cd']
b=eoshift(a,1,'--')
print *,len(b),size(b)
print '(2(a,a,a))','[',b(1),']','[',b(2),']'
b=unpack(a,[.true.,.false.,.true.],'--')
print *,len(b),size(b)
print '(3(a,a,a))','[',b(1),']','[',b(2),']','[',b(3),']'
end
""", '2 2\n[cd][--]\n2 3\n[ab][--][cd]\n',interpreter=False)

    def test_sections_and_resized_matrix(self):
        self.check("""implicit none
integer::n
character(2),allocatable::a(:,:),b(:,:)
n=2
allocate(a(-1:0,4:6))
a=reshape(['a ','b ','c ','d ','e ','f '],[2,3])
b=cshift(a(:,6:4:-2),1,dim=2)
print *,shape(b)
print '(4a)',b
b=eoshift(a,1,dim=1)
print *,shape(b)
print '(6a)',b
b=a
print *,lbound(b)
deallocate(b)
b=a
print *,lbound(b),ubound(b)
end
""", '2 2\na b e f \n2 3\nb   d   f   \n1 1\n-1 4 0 6\n')

    def test_deferred_section(self):
        program="""implicit none
integer::n
character(:),allocatable::a(:),b(:),reversed(:)
n=2
allocate(character(len=n)::a(3))
a=['ab','cd','ef']
b=cshift(a(3:1:-1),1)
print *,len(b),size(b)
print '(3(a,a,a))','[',b(1),']','[',b(2),']','[',b(3),']'
end
"""
        expected='2 3\n[cd][ab][ef]\n'
        # Installed gfortran produces corrupt characters for the direct reversed
        # deferred-length section. Compare using an explicitly captured section.
        self.check(program,expected,interpreter=False,reference=False)
        self.check(program.replace('b=cshift(a(3:1:-1),1)',
            'reversed=a(3:1:-1)\nb=cshift(reversed,1)'),expected,interpreter=False)

    def test_fixed_matrix_to_allocatable_result(self):
        self.check("""implicit none
character(1)::a(2,2)
character(1),allocatable::b(:,:)
a(1,1)='a'
a(2,1)='b'
a(1,2)='c'
a(2,2)='d'
b=cshift(a,1,dim=2)
print *,shape(b)
print '(4a)',b
end
""",'2 2\ncdab\n')

    def test_deferred_procedure_argument(self):
        self.check("""implicit none
integer::n
character(:),allocatable::a(:)
n=2
allocate(character(len=n)::a(2))
a=['ab','cd']
call show(cshift(a,1))
contains
subroutine show(x)
character(*),intent(in)::x(:)
print *,len(x),size(x)
print '(2(a,a,a))','[',x(1),']','[',x(2),']'
end subroutine
end
""", '2 2\n[cd][ab]\n',interpreter=False)

    def test_length_mismatch(self):
        self.check("""implicit none
integer::n
character(:),allocatable::a(:)
n=2
allocate(character(len=n)::a(2))
a=['ab','cd']
print *,eoshift(a,1,'x')
end
""",error='CHARACTER lengths must match',interpreter=False)

    def test_empty_and_zero_length(self):
        self.check("""implicit none
integer::n
character(0),allocatable::a(:),b(:)
n=2
allocate(a(n))
a=''
b=cshift(a,1)
print *,len(b),size(b)
b=eoshift(a,1)
print *,len(b),size(b)
deallocate(a)
allocate(a(0))
b=unpack(a,[.false.,.false.],'')
print *,len(b),size(b)
b=cshift(a,1)
print *,len(b),size(b)
end
""", '0 2\n0 2\n0 2\n0 0\n')

if __name__=='__main__':unittest.main()

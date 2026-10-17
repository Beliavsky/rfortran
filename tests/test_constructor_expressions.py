"""Runtime implied-DO constructors used as ordinary expression values."""
import unittest
import subprocess
import tempfile
from pathlib import Path
import test_deallocate
from test_record_read import ROOT, build_commands

class ConstructorExpressions(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_reproducer(self):
        self.check((ROOT/'tests/programs/xconstructor_arithmetic.f90').read_text(), '100 137 174\n')

    def test_runtime_input_and_arithmetic(self):
        self.check('''integer::i,n,a(3)
read(*,*)n
a=100+37*[(i-1,i=1,n)]
print *,a,sum(2*[(i,i=1,n)])
end
''','100 137 174 12\n',data='3\n')

    def test_nested_bounds_index_scope_and_stride(self):
        self.check('''integer::i,j,n,s
read(*,*)n
read(*,*)s
i=77
j=88
print *,2*[((10*i+j,j=1,i),i=1,n)],i,j
print *,10+[(i,i=n,1,s)],i
end
''','22 42 44 62 64 66 77 88\n13 11 77\n',data='3\n-2\n')

    def test_empty_and_repeated_evaluation(self):
        self.check('''integer::i,n,k
read(*,*)n
do k=0,n
print *,sum(10*[(i,i=1,k)]),size([(i,i=1,k)])
end do
end
''','0 0\n10 1\n30 2\n60 3\n',data='3\n')

    def test_procedure_argument(self):
        self.check('''integer::i,n
read(*,*)n
call show(100+[(i,i=1,n)])
contains
subroutine show(x)
integer,intent(in)::x(:)
print *,x,size(x)
end subroutine
end
''','101 102 103 3\n',data='3\n')

    def test_while_condition(self):
        self.check('''integer::i,n
n=3
do while(sum([(i,i=1,n)])>0)
print *,n
n=n-1
end do
end
''','3\n2\n1\n')

    def test_types_and_kinds(self):
        self.check('''integer::n
integer(8)::i
read(*,*)n
print *,sum(2_8*[(i,i=1,n)])
print *,sum(0.5*[(real(i),i=1,n)])
print *,sum(0.25d0*[(dble(i),i=1,n)])
print *,any([(i>1,i=1,n)])
end
''','12\n3.0\n1.5\nT\n',data='3\n')

    def test_controls_and_items_evaluated_once(self):
        self.check("""module counter_mod
integer::calls=0,items=0
contains
integer function bound(k) result(v)
integer,intent(in)::k
calls=calls+1
v=k
end function
integer function value(k,l) result(v)
integer,intent(in)::k,l
items=items+1
v=10*k+l
end function
end module
program main
use counter_mod
integer::i,j,n
n=3
print *,2*[((value(i,j),j=1,bound(i)),i=1,n)]
print *,calls,items
end
""",'22 42 44 62 64 66\n3 6\n')

    def test_array_items(self):
        self.check("""integer::i,n,a(2),b(2,2)
a=[1,2]
b=reshape([3,4,5,6],[2,2])
read(*,*)n
print *,10*[(a,i=1,n)]
print *,[(b,i=1,n)]
end
""",'10 20 10 20\n3 4 5 6 3 4 5 6\n',data='2\n')

    def test_character_items(self):
        self.check("""integer::i,n
character(2)::a(3)
read(*,*)n
a=[('hi',i=1,n)]
print *,a=='hi'
end
""",'T T T\n',data='3\n')

    def test_runtime_zero_stride(self):
        self.check("""integer::i,n,step
n=3
read(*,*)step
print *,2*[(i,i=1,n,step)]
end
""",error='constructor implied-DO stride must not be zero',data='0\n')

    def test_constructor_inside_io_implied_do(self):
        self.check("""integer::i,j,n
read(*,*)n
print *, (sum([(i+j,j=1,n)]),i=1,n)
print '(3(1x,i0))', (sum([(i+j,j=1,n)]),i=1,n)
end
""",'9 12 15\n9 12 15\n',data='3\n')

    def test_size_retains_default_integer_kind(self):
        self.check("""integer::i,n
read(*,*)n
print *,size([(i,i=1,n)]),kind(size([(i,i=1,n)]))
end
""",'3 4\n',data='3\n')

    def test_native_growth_beyond_previous_bounded_storage(self):
        # Keep the large allocation check native: interpreter frame snapshots
        # make large element-by-element loops disproportionately expensive.
        with tempfile.TemporaryDirectory(prefix='constructor-growth-') as tmp:
            directory=Path(tmp)
            source=directory/'growth.f90'
            source.write_text("""integer::i,n,k
n=70000
do k=1,3
print *,sum(1_8*[(1_8,i=1,n)])
end do
end
""")
            for command in build_commands(source,directory)[1:]:
                with self.subTest(command=command):
                    result=subprocess.run(command,capture_output=True,text=True,timeout=30)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stderr,'')
                    self.assertEqual(result.stdout.split(),['70000']*3)

if __name__=='__main__':
    unittest.main(verbosity=2)

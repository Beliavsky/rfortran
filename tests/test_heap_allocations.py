"""Heap allocation with runtime dimensions, shared storage and cleanup."""
import unittest
import subprocess
import tempfile
from pathlib import Path
import test_deallocate
from test_record_read import ROOT,build_commands

class HeapAllocations(unittest.TestCase):
    check=test_deallocate.Deallocate.check

    def test_function_computed_module_shape(self):
        self.check((ROOT/'tests/175_heap_allocations.f90').read_text(encoding='utf-8'),'6 2 12\n')

    def test_input_sized_vector_and_matrix(self):
        self.check("""integer::n
integer,allocatable::v(:),a(:,:)
read (*,*) n
allocate(v(n),a(n,2))
v=3
a=4
print *,size(v),sum(v),shape(a),sum(a)
deallocate(v,a)
end
""",'9 27 9 2 72\n',data='9\n')

    def test_input_sized_rank_three(self):
        self.check("""integer::n
real(8),allocatable::a(:,:,:)
read (*,*) n
allocate(a(n,2,3))
a=2.5d0
a(n,2,3)=7.5d0
print *,shape(a),sum(a),a(n,2,3)
deallocate(a)
end
""",'4 2 3 65 7.5\n',data='4\n')

    def test_rank_fifteen(self):
        self.check('integer::n\nlogical,allocatable::a('+','.join([':']*15)+')\n'
                   'read (*,*) n\nallocate(a(n,'+','.join(['1']*14)+'))\n'
                   'a=.true.\nprint *,size(a),all(a)\ndeallocate(a)\nend\n',
                   '3 T\n',data='3\n')

    def test_shared_reallocation_and_initialization(self):
        self.check("""module m
integer,allocatable::a(:,:)
contains
subroutine fill(n)
integer,intent(in)::n
if(allocated(a)) deallocate(a)
allocate(a(n,2))
a=5
end subroutine fill
end module
use m
integer::n
read (*,*) n
call fill(n)
print *,shape(a),sum(a)
call fill(n+2)
print *,shape(a),sum(a)
deallocate(a)
end
""",'3 2 30\n5 2 50\n',data='3\n')

    def test_assumed_shape_mutation(self):
        self.check("""program main
integer::n
integer,allocatable::a(:,:)
read (*,*) n
allocate(a(n,2))
a=1
call change(a)
print *,sum(a),a(n,2)
deallocate(a)
contains
subroutine change(b)
integer,intent(inout)::b(:,:)
b(size(b,1),size(b,2))=8
end subroutine change
end program main
""",'13 8\n',data='3\n')

    def test_local_cleanup_and_early_return(self):
        self.check("""program main
integer::n,i,total
read (*,*) n
do i=1,3
call work(n,total)
print *,total
end do
contains
subroutine work(n,total)
integer,intent(in)::n
integer,intent(out)::total
integer,allocatable::a(:,:)
allocate(a(n,2))
a=6
total=sum(a)
return
end subroutine work
end program main
""",'48\n48\n48\n',data='4\n')

    def test_runtime_lower_bounds_and_dummy_rebasing(self):
        self.check("""program main
integer::n
integer,allocatable::a(:,:)
read (*,*) n
allocate(a(-2:n,0:1))
a=4
a(-2,0)=9
print *,shape(a),lbound(a),ubound(a),sum(a)
call show(a)
deallocate(a)
contains
subroutine show(b)
integer,intent(in)::b(:,:)
print *,lbound(b),ubound(b),b(1,1)
end subroutine show
end program main
""",'6 2 -2 0 3 1 53\n1 1 6 2 9\n',data='3\n')

    def test_zero_extent_avoids_product_overflow(self):
        # A zero-sized shape needs no element storage. This gfortran build
        # rejects the overflowing intermediate product, so check rfort directly.
        program="""integer(8)::n
integer,allocatable::a(:,:,:)
read (*,*) n
allocate(a(n,n,0))
print *,allocated(a),size(a),sum(a)
deallocate(a)
end
"""
        with tempfile.TemporaryDirectory(prefix='heap-empty-') as tmp:
            directory=Path(tmp)
            source=directory/'example.f90'
            source.write_text(program,encoding='utf-8')
            for command in build_commands(source,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,input='9223372036854775807\n',
                                          capture_output=True,text=True,timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stderr,'')
                    self.assertEqual(result.stdout.split(),['T','0','0'])

    def test_shared_module_lower_bounds(self):
        self.check("""module m
integer,allocatable::a(:,:)
contains
subroutine fill(n)
integer,intent(in)::n
allocate(a(-2:n,0:1))
a=1
end subroutine fill
end module m
use m
integer::n
read (*,*) n
call fill(n)
print *,lbound(a),ubound(a),sum(a)
a(-2,0)=7
print *,sum(a)
deallocate(a)
end
""",'-2 0 3 1 12\n18\n',data='3\n')

    def test_overflow_and_uninitialized_elements(self):
        self.check("""integer(8)::n
integer,allocatable::a(:,:)
read (*,*) n
allocate(a(n,3))
end
""",error='allocation size overflow',data='9223372036854775807\n')
        self.check("""integer::n
integer,allocatable::a(:,:)
read (*,*) n
allocate(a(n,2))
a(1,1)=2
print *,a(2,1)
end
""",error='uninitialized',data='3\n')

if __name__=='__main__':unittest.main()

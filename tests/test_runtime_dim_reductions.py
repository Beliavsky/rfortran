"""Runtime DIM reductions, compared with gfortran in both rfort execution modes."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import test_deallocate
from test_record_read import ROOT


class RuntimeDimReductions(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def compare(self, program):
        if not shutil.which("gfortran"):
            self.skipTest("gfortran required for differential tests")
        with tempfile.TemporaryDirectory(prefix="runtime-dim-reference-") as tmp:
            source = Path(tmp) / "main.f90"
            exe = Path(tmp) / "reference.exe"
            source.write_text(program)
            result = subprocess.run(["gfortran", "-fcheck=all", source, "-o", exe],
                                    cwd=tmp, capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([exe], cwd=tmp, capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.check(program, result.stdout, reference=False)

    def test_xacv_repro(self):
        self.check((ROOT / "tests/programs/xacv_runtime_dim_reduction.f90").read_text(),
                   "11 21 31\n31 32\n23 43 63\n")

    def test_numeric_reductions_and_masks(self):
        self.compare("""implicit none
integer,allocatable::a(:,:,:)
logical,allocatable::m(:,:,:)
integer::n,i,j,k
n=2
allocate(a(-1:0,3:5,2:n+1),m(n,3,n))
do k=2,n+1
do j=3,5
do i=-1,0
a(i,j,k)=i+2+j+2*k
end do
end do
end do
m=mod(a,2)==0
print *,sum(a,dim=1),sum(a,dim=2),sum(a,dim=3)
print *,product(a,dim=1),product(a,dim=2),product(a,dim=3)
print *,minval(a,dim=1),minval(a,dim=2),minval(a,dim=3)
print *,maxval(a,dim=1),maxval(a,dim=2),maxval(a,dim=3)
print *,sum(array=a,mask=m,dim=2),product(a,mask=m,dim=3)
print *,minval(a,mask=.false.,dim=2),maxval(a,dim=1,mask=.false.)
print *,sum(a,mask=m),sum(a,m),sum(a,mask=.false.)
print *,sum(sum(a,dim=2),dim=1)
end
""")

    def test_logical_and_bit_reductions(self):
        self.compare("""implicit none
integer,allocatable::a(:,:)
logical,allocatable::m(:,:)
integer::n
n=2
allocate(a(n,3),m(n,3))
a=reshape([1,2,3,4,5,6],[2,3])
m=mod(a,2)==0
print *,any(m,dim=1),all(m,dim=2),parity(m,dim=1)
print *,count(mask=m,dim=1),count(m,dim=2,kind=8)
print *,iany(a,dim=1),iall(a,dim=2),iparity(a,dim=1)
print *,iany(a,dim=2,mask=m),iall(a,dim=1,mask=m.and..false.)
end
""")

    def test_rank_one_scalar_results(self):
        self.compare("""implicit none
real,allocatable::a(:)
logical,allocatable::m(:)
integer::n
n=3
allocate(a(n),m(n))
a=[3.,4.,0.]
m=a>0
print *,sum(a,dim=1),product(a,dim=1),minval(a,dim=1),maxval(a,dim=1)
print *,sum(a,dim=1,mask=m),count(m,dim=1),any(m,dim=1),all(m,dim=1),parity(m,dim=1)
print *,norm2(a,dim=1)
end
""")

    def test_empty_axes_and_masked_uninitialized_elements(self):
        self.compare("""implicit none
integer,allocatable::a(:,:),b(:,:)
logical,allocatable::m(:,:)
integer::n
n=0
allocate(a(n,3),b(2,2),m(2,2))
print *,sum(a,dim=1),product(a,dim=1),minval(a,dim=1),maxval(a,dim=1)
print *,size(sum(a,dim=2))
m=.false.
m(1,1)=.true.
m(2,2)=.true.
b(1,1)=7
b(2,2)=9
print *,sum(b,mask=m,dim=1)
end
""")

    def test_result_assignment_and_norm2(self):
        self.compare("""implicit none
double precision,allocatable::a(:,:),v(:),b(:,:)
double precision::fixed(3)
integer::n
n=2
allocate(a(n,3))
a=3d0
v=sum(a,dim=1)
fixed=minval(a,dim=1)
print *,v,fixed,norm2(a,dim=1),norm2(a,dim=2)
print *,size(v),shape(v)
end
""")

    def test_runtime_locations(self):
        self.compare("""implicit none
integer,allocatable::a(:,:,:),v(:,:)
logical,allocatable::mask(:,:,:)
integer::n
n=2
allocate(a(n,3,n),mask(n,3,n))
a=reshape([4,2,1,3,1,2,4,2,1,3,1,2],[2,3,2])
mask=a>1
print *,minloc(a,dim=1),minloc(a,dim=2),minloc(a,dim=3)
print *,maxloc(a,dim=1),maxloc(a,dim=2),maxloc(a,dim=3)
print *,findloc(a,1,dim=1),findloc(a,1,dim=2),findloc(a,1,dim=3)
print *,minloc(a,back=.true.),maxloc(a,back=.true.),findloc(a,1,back=.true.)
print *,minloc(a,dim=2,mask=mask,back=.true.,kind=8)
print *,maxloc(a,dim=1,mask=.false.),findloc(a,7,dim=3)
print *,findloc(mask,.true.,dim=2,back=.true.)
v=minloc(a,dim=3)
print *,v,shape(v)
end
""")

    def test_empty_locations_and_rank_one(self):
        self.compare("""implicit none
real,allocatable::a(:,:),b(:)
logical,allocatable::m(:,:)
integer::n
n=0
allocate(a(n,3),b(3),m(n,3))
b=[2.,1.,1.]
print *,minloc(a,dim=1),maxloc(a,dim=1),findloc(a,0.,dim=1)
print *,minloc(a),maxloc(a),findloc(a,0.)
print *,minloc(b,dim=1,back=.true.),maxloc(b,dim=1),findloc(b,1.,dim=1)
print *,minloc(b),maxloc(b),findloc(b,1.,back=.true.,kind=8)
end
""")

    def test_kinds_and_sections(self):
        for decl, value in [("integer(1)","2"),("integer(2)","2"),("integer(8)","2"),
                            ("real","2."),("double precision","2d0")]:
            with self.subTest(decl=decl):
                self.compare(f"""implicit none
{decl},allocatable::a(:,:,:)
integer::n
n=2
allocate(a(n,3,n))
a={value}
print *,sum(a,dim=1),product(a,dim=2),minval(a,dim=3),maxval(a,dim=2)
print *,kind(sum(a,dim=1)),sum(a(2:1:-1,3:1:-2,:),dim=2)
print *,minloc(a(2:1:-1,3:1:-2,:),dim=2,back=.true.)
end
""")

    def test_assumed_shape_dummy_and_loop_evaluation(self):
        self.compare("""program main
implicit none
integer,allocatable::a(:,:)
integer::n
n=2
allocate(a(n,3))
a=1
call check(a)
contains
subroutine check(x)
integer,intent(inout)::x(:,:)
do while (any(sum(x,dim=1)<5))
x=x+1
end do
print *,sum(x,dim=1)
print *,maxloc(x,dim=2,back=.true.)
end subroutine
end program
""")

    def test_fixed_mask_and_runtime_source(self):
        self.compare("""integer,allocatable::a(:,:)
logical::m(2,3)
integer::n
n=2
allocate(a(n,3))
a=2
m=.true.
m(1,2)=.false.
print *,sum(a,dim=1,mask=m),product(a,dim=2,mask=m)
print *,minloc(a,dim=1,mask=m),findloc(a,2,dim=2,mask=m)
end
""")

    def test_ranks_two_through_fifteen(self):
        for rank in (2,3,7,15):
            with self.subTest(rank=rank):
                dims=":"+",:"*(rank-1)
                extents=",".join(["n"]+["1"]*(rank-2)+["3"])
                self.compare(f"""integer,allocatable::a({dims})
integer::n
n=2
allocate(a({extents}))
a=3
print *,sum(a,dim=1),sum(a,dim={rank})
print *,minloc(a,dim={rank},back=.true.)
print *,shape(sum(a,dim=1))
end
""")

    def test_positional_constructor_masks(self):
        self.compare("""integer,allocatable::a(:)
integer::n
n=3
allocate(a(n))
a=[1,2,3]
print *,sum(a,[.true.,.false.,.true.]),product(a,.false.)
print *,iall(a,[.true.,.true.,.false.])
print *,minloc(a,[.false.,.true.,.true.])
print *,findloc(a,3,[.true.,.false.,.true.])
end
""")

    def test_reallocation_and_repeated_evaluation(self):
        self.compare("""integer,allocatable::a(:,:,:),b(:,:)
integer::n,i
n=2
do i=1,2
allocate(a(n,3,i))
a=i
b=sum(a,dim=2)
print *,shape(b),b
print *,minloc(a,dim=3,back=.true.)
deallocate(a)
n=n+1
end do
end
""")

    def test_empty_iall_identity(self):
        # gfortran's scalar MASK=.false. path differs from its array-MASK
        # path here; test the all-one-bits identity directly.
        self.check("""integer,allocatable::a(:,:)
integer::n
n=0
allocate(a(n,3))
print *,iall(a,dim=1),iany(a,dim=1),iparity(a,dim=1)
print *,iall(a,dim=1,mask=.false.)
end
""", "-1 -1 -1 0 0 0 0 0 0\n-1 -1 -1\n", reference=False)

    def test_invalid_dim_and_mask_shape(self):
        for dim in ["0","3"]:
            with self.subTest(dim=dim):
                self.reject(f"""real,allocatable::a(:,:)
allocate(a(2,3))
a=1
print *,sum(a,dim={dim})
end
""", "DIM must be between 1 and 2")
        self.check("""integer,allocatable::a(:,:)
logical,allocatable::m(:,:)
allocate(a(2,3),m(3,2))
a=1
m=.true.
print *,sum(a,dim=1,mask=m)
end
""", error="shapes do not conform", reference=False)
        self.reject("""integer,allocatable::a(:,:)
print *,minval(a,dim=1)
end
""", "used before ALLOCATE")
        self.check("""integer,allocatable::a(:)
allocate(a(3))
a=1
print *,minloc(a,mask=[.true.,.false.])
end
""", error="shape does not conform", reference=False)
        self.check("""integer,allocatable::a(:,:)
logical::allocate_it
allocate_it=.false.
if (allocate_it) allocate(a(2,3))
print *,minval(a,dim=1)
end
""", error="unallocated", reference=False)


if __name__ == "__main__":
    unittest.main()

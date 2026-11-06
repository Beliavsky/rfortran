"""MOVE_ALLOC and the SOURCE=, MOLD=, STAT= and ERRMSG= allocation options.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_allocation_options.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class AllocationOptions(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_source_preserves_explicit_bounds(self):
        self.check('''implicit none
integer,allocatable::a(:)
allocate(a(-1:1),source=[2,3,4])
print *,lbound(a),a
end
''','-1 2 3 4')

    def test_source_allocation_inquiry_is_evaluated_before_allocate(self):
        self.check('''implicit none
logical,allocatable::a(:)
allocate(a(2),source=allocated(a))
print *,a
end
''','F F')

    def test_recoverable_size_and_extent_overflow(self):
        from pathlib import Path
        self.check((Path(__file__).parent/'programs/xallocation_status_overflow.f90').read_text(),
                   'T F T\nT T 0\nT F\nT F\nT 7 7 7',reference=False)

    def test_status_destination_in_source_and_bounds(self):
        self.check("""implicit none
integer,allocatable::a(:),b(:)
integer::s
s=3
allocate(a(s),source=s,stat=s)
print *,s,a
s=2
allocate(b(-s:s),source=s+1,stat=s)
print *,s,lbound(b),b
deallocate(a,b)
end
""",'0 3 3 3 0 -2 3 3 3 3 3')

    def test_rank_fifteen_status_and_multiple_targets(self):
        dims=','.join([':']*15)
        huge_shape=','.join(['n','n']+['1']*13)
        small_shape=','.join(['2']+['1']*14)
        self.check(f"""implicit none
logical,allocatable::a({dims})
integer,allocatable::b(:)
integer(kind=8)::n
integer::s
n=huge(n)/2
allocate(a({huge_shape}),stat=s)
print *,s/=0,allocated(a)
allocate(a({small_shape}),b(n),stat=s)
print *,s/=0,allocated(a),allocated(b),size(a)
a=.true.
print *,all(a)
deallocate(a)
end
""",'T F T T F 2 T',reference=False)

    def test_injected_allocator_failures_release_partial_buffers(self):
        # Rename only this object's allocator imports. The test shim delegates
        # to the real C allocator and leaves runtime/library allocations alone.
        import os
        import subprocess
        import tempfile
        from pathlib import Path
        from test_c_interop import C_COMPILER
        from test_record_read import COMPILER
        if not C_COMPILER:
            self.skipTest('a C compiler is required for allocator injection')
        source = """use iso_c_binding
implicit none
interface
 subroutine arm(n) bind(c)
 import c_int
 integer(c_int),value::n
 end subroutine
 integer(c_int) function live() bind(c)
 import c_int
 end function
end interface
integer,allocatable::a(:)
integer::s,i
character(len=40)::message
do i=1,2
 call arm(i)
 allocate(a(3),source=7,stat=s,errmsg=message)
 print *,s/=0,allocated(a),live(),len_trim(message)>0
end do
call arm(0)
allocate(a(3),source=7,stat=s)
print *,s==0,live(),a
deallocate(a)
print *,live()
end
"""
        shim = """typedef __SIZE_TYPE__ size_t;
extern void *malloc(size_t);
extern void free(void *);
extern void *memset(void *,int,size_t);
static int attempt, fail_at, count;
void arm(int n) { attempt=0; fail_at=n; }
int live(void) { return count; }
void *talloc(size_t n,size_t w) {
    if (++attempt==fail_at) return 0;
    void *p=malloc(n*w);
    if(p) { memset(p,0,n*w); ++count; }
    return p;
}
void tfre(void *p) { if(p) { --count; free(p); } }
"""
        with tempfile.TemporaryDirectory(prefix='rfort-allocation-failure-') as temp:
            d=Path(temp)
            (d/'case.f90').write_text(source)
            (d/'shim.c').write_text(shim)
            flags=['--target=x86_64-pc-windows-msvc'] if os.name=='nt' and 'clang' in Path(C_COMPILER).name else []
            def run(command):
                p=subprocess.run(list(map(str,command)),cwd=d,capture_output=True,text=True,timeout=90)
                self.assertEqual(p.returncode,0,p.stdout+p.stderr)
                return p
            run([C_COMPILER,*flags,'-c','shim.c','-o','shim.obj'])
            for kind,value,printed in [('integer','7','7'),('real(kind=8)','7.0_8','7.0'),('logical','.true.','T')]:
                (d/'case.f90').write_text(source.replace('integer,allocatable',kind+',allocatable').replace('source=7','source='+value))
                for opt in ('0','2'):
                    with self.subTest(kind=kind,opt=opt):
                        run([COMPILER,'-O',opt,'-c','case.f90','-o','case.obj'])
                        obj=d/'case.obj'
                        code=obj.read_bytes()
                        self.assertIn(b'calloc\0',code)
                        self.assertIn(b'free\0',code)
                        obj.write_bytes(code.replace(b'calloc\0',b'talloc\0').replace(b'free\0',b'tfre\0'))
                        run([COMPILER,'link','case.obj','shim.obj','-o','case.exe'])
                        p=run([d/'case.exe'])
                        self.assertEqual(p.stdout.split(),('T F 0 T T F 0 T T 2 '+(' '.join([printed]*3))+' 0').split())

    def test_invalid_controls(self):
        for statement,message in (
            ('allocate(a(2),stat=s,stat=s)','duplicate ALLOCATE option'),
            ('allocate(a,source=[1,2],mold=[1,2])','mutually exclusive'),
            ('deallocate(a,stat=s,stat=s)','duplicate DEALLOCATE option')):
            with self.subTest(statement=statement):
                self.reject('implicit none\ninteger,allocatable::a(:)\ninteger::s\n'+statement+'\nend\n',message)

    def test_move_preserves_numeric_initialization_flags(self):
        for kind,value in [('integer','7'),('real','1.0'),('logical','.true.')]:
            with self.subTest(kind=kind):
                self.check(f'''implicit none
{kind},allocatable::a(:),b(:)
integer::n
read *,n
allocate(a(n))
a(1)={value}
call move_alloc(from=a,to=b)
print *,b
end
''',error='uninitialized variable `b`: element 2',data='3\n')

    def test_options_and_move_alloc(self):
        self.check("""program al2
implicit none
type :: t
  integer, allocatable :: a(:)
end type t
type(t) :: x, y
integer, allocatable :: a(:), b(:), c(:,:), d(:,:)
integer :: s
character(len=40) :: m
allocate(a(-1:1), stat=s, errmsg=m)
print *, s == 0, lbound(a), size(a)
a = [4, 5, 6]
allocate(a(2), stat=s, errmsg=m)
print *, s /= 0, len_trim(m) > 0
allocate(b, source=a)
print *, lbound(b), b
deallocate(b)
allocate(b, mold=a)
print *, size(b), lbound(b)
call move_alloc(a, b)
print *, allocated(a), allocated(b), lbound(b), b
deallocate(a, stat=s)
print *, s /= 0
allocate(c(2,3))
c = 7
call move_alloc(c, d)
print *, allocated(c), shape(d), sum(d)
allocate(x%a(2))
x%a = [1, 2]
call move_alloc(x%a, y%a)
print *, allocated(x%a), y%a
end program al2
""", 'T -1 3\nT T\n-1 4 5 6\n3 -1\nF T -1 4 5 6\nT\nF 2 3 42\nF 1 2\n')


if __name__ == "__main__":
    unittest.main()

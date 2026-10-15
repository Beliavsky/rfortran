"""Recursive array descriptors: interpreter, native O0/O2, and gfortran."""
from pathlib import Path
import os
import subprocess
import tempfile
import unittest
from test_record_read import ROOT, COMPILER, build_commands
import test_recursive_allocatables


class RecursiveArrays(unittest.TestCase):
    compare = test_recursive_allocatables.RecursiveAllocatables.compare

    def test_original_reproducer(self):
        self.compare((ROOT / 'xrecursive_optional_array.f90').read_text(), '1 2 3\n')

    def test_separately_compiled_recursive_module(self):
        text=(ROOT/'xrecursive_optional_array.f90').read_text()
        module,main=text.split('program test_recursive_optional_array',1)
        main='program test_recursive_optional_array'+main
        with tempfile.TemporaryDirectory(prefix='recursive-array-modules-') as tmp:
            directory=Path(tmp)
            module_path=directory/'generator.f90'
            main_path=directory/'main.f90'
            module_path.write_text(module)
            main_path.write_text(main)
            for name in ['generator','main']:
                result=subprocess.run([COMPILER,'-c',directory/(name+'.f90')],cwd=directory,
                                      capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(result.stderr,'')
                if name=='generator': module_path.unlink()
            extension='.obj' if os.name=='nt' else '.o'
            exe=directory/('main.exe' if os.name=='nt' else 'main')
            result=subprocess.run([COMPILER,directory/('generator'+extension),directory/('main'+extension),
                                   '-o',exe,'--run'],cwd=directory,capture_output=True,text=True,timeout=60)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stderr,'')
            self.assertEqual(result.stdout.split(),['1','2','3'])

    def test_explicit_shape_combinations_and_saved_local(self):
        self.compare('''module m
implicit none
contains
recursive subroutine gen(m,nmax,mmax,comb,mat,reset)
integer,intent(in)::m,nmax,mmax
integer,intent(inout)::comb(mmax),mat(:,:)
logical,intent(in),optional::reset
integer::n
integer,save::row=0
if(present(reset))then
   if(reset)row=0
end if
if(m>mmax)then
   row=row+1
   mat(row,:)=comb
else
   do n=1,nmax
      if(m>1)then
         if(n<=comb(m-1))cycle
      end if
      comb(m)=n
      call gen(m+1,nmax,mmax,comb,mat)
   end do
end if
end subroutine
end module
program main
use m
implicit none
integer,allocatable::comb(:),mat(:,:)
integer::n,k,i
n=4
k=2
allocate(comb(k),mat(6,k))
comb=0
call gen(1,n,k,comb,mat,reset=.true.)
do i=1,size(mat,1)
print *,mat(i,:)
end do
call gen(1,n,k,comb,mat,reset=.true.)
print *,sum(mat)
end
''', '1 2\n1 3\n1 4\n2 3\n2 4\n3 4\n30\n')

    def test_nonrecursive_allocatable_wrapper_keeps_specialization(self):
        self.compare('''module m
contains
recursive subroutine fill(i,a)
integer,intent(in)::i
integer,intent(inout)::a(:)
if(i>size(a))return
a(i)=i
call fill(i+1,a)
end subroutine
subroutine make(n,a)
integer,intent(in)::n
integer,allocatable,intent(out)::a(:)
allocate(a(n))
call fill(1,a)
end subroutine
end module
program main
use m
integer,allocatable::a(:)
allocate(a(1))
a=99
call make(3,a)
print *,a
end
''', '1 2 3\n')

    def test_mutual_recursion_and_writable_scalar(self):
        self.compare('''module m
implicit none
contains
recursive subroutine odd(i,a,total)
integer,intent(in)::i
integer,intent(inout)::a(:),total
integer::saved
saved=i*10
if(i<=size(a))then
   a(i)=i
   call even(i+1,a,total)
end if
total=total+saved
end subroutine
recursive subroutine even(i,a,total)
integer,intent(in)::i
integer,intent(inout)::a(:),total
integer::saved
saved=i*10
if(i<=size(a))then
   a(i)=i
   call odd(i+1,a,total)
end if
total=total+saved
end subroutine
end module
program main
use m
implicit none
integer::a(4),total
total=0
call odd(1,a,total)
print *,a,total
end
''', '1 2 3 4 150\n')

    def test_runtime_rank_three_arrays_and_overlapping_assignments(self):
        self.compare('''module m
implicit none
contains
recursive subroutine update(n,a,flags)
integer,intent(in)::n
real(8),intent(inout)::a(:,:,:)
logical,intent(inout)::flags(:,:,:)
if(n<=0)return
a=a+1.0d0
flags=.not.flags
call update(n-1,a,flags)
a(:,:,2:3)=a(:,:,1:2)
end subroutine
end module
program main
use m
implicit none
real(8),allocatable::a(:,:,:)
logical,allocatable::flags(:,:,:)
integer::n
n=2
allocate(a(n,n,3),flags(n,n,3))
a=1.0d0
flags=.false.
call update(3,a,flags)
print *,sum(a),count(flags)
end
''', '48.0 12\n')

    def test_runtime_sections_and_intent_out(self):
        self.compare('''module m
implicit none
contains
subroutine start(a)
integer,intent(out)::a(:)
call fill(1,a)
end subroutine
recursive subroutine fill(i,a)
integer,intent(in)::i
integer,intent(inout)::a(:)
if(i>size(a))return
a(i)=i
call fill(i+1,a)
end subroutine
end module
program main
use m
implicit none
integer,allocatable::a(:,:)
integer::n
n=5
allocate(a(2,n))
a(1,:)=-1
call start(a(2,5:1:-2))
print *,a(2,5:1:-2),a(1,:)
end
''', '1 2 3 -1 -1 -1 -1 -1\n')

    def test_runtime_depth_and_empty_array(self):
        self.compare('''module m
implicit none
contains
recursive subroutine fill(i,a)
integer,intent(in)::i
integer,intent(inout)::a(:)
if(i>size(a))return
a(i)=i
call fill(i+1,a)
end subroutine
end module
program main
use m
implicit none
integer,allocatable::a(:)
integer::n
read (*,*)n
allocate(a(n))
call fill(1,a)
print *,sum(a)
deallocate(a)
allocate(a(0))
call fill(1,a)
print *,size(a)
end
''', '703\n0\n', '37\n')

    def test_intent_out_is_reset_at_each_recursive_entry(self):
        self.compare('''module m
contains
recursive subroutine fill(n,a)
integer,intent(in)::n
integer,intent(out)::a(:)
a=n
if(n>0)call fill(n-1,a)
end subroutine
end module
program main
use m
integer::a(3)
call fill(3,a)
print *,a
end
''', '0 0 0\n')

    def test_recursive_intent_out_does_not_retain_old_initialization(self):
        source='''module m
contains
recursive subroutine fill(n,a)
integer,intent(in)::n
integer,intent(out)::a(:)
if(n>0)then
   a(1)=n
   call fill(n-1,a)
else
   a(size(a))=0
end if
end subroutine
end module
program main
use m
integer::a(3)
a=9
call fill(3,a)
print *,a
end
'''
        self.failure(source,1)

    def test_explicit_bound_dummy_after_array(self):
        self.compare('''module m
contains
recursive subroutine fill(a,n,i)
integer,intent(in)::n,i
integer,intent(inout)::a(n)
if(i>n)return
a(i)=i
call fill(a,n,i+1)
end subroutine
end module
program main
use m
integer::a(3)
call fill(a,3,1)
print *,a
end
''', '1 2 3\n')

    def test_rank_fifteen(self):
        dims=','.join(':' for _ in range(15))
        bounds=','.join(['1']*14+['n'])
        self.compare(f'''module m
contains
recursive subroutine fill(i,a)
integer,intent(in)::i
real(8),intent(inout)::a({dims})
if(i>0)then
call fill(i-1,a)
else
a=5.0d0
end if
end subroutine
end module
program main
use m
real(8),allocatable::a({dims})
integer::n
n=2
allocate(a({bounds}))
call fill(2,a)
print *,sum(a)
end
''', '10.0\n')

    def test_forwarding_keeps_type_kind_and_readonly_checks(self):
        for declarations, expected in [
            ('integer,intent(in)::a(:)\ninteger,intent(inout)::b(:)', 'INTENT(IN)'),
            ('real,intent(inout)::a(:)\ninteger,intent(inout)::b(:)', 'type or kind mismatch'),
        ]:
            source=f'''module m
contains
recursive subroutine visit(i,a,b)
integer,intent(in)::i
{declarations}
if(i>0)call visit(i-1,a,a)
end subroutine
end module
end
'''
            with tempfile.TemporaryDirectory(prefix='recursive-array-type-') as tmp:
                path=Path(tmp)/'bad.f90'
                path.write_text(source)
                result=subprocess.run([COMPILER,'check',path],capture_output=True,text=True,timeout=30)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(expected,result.stderr)

    def test_uninitialized_element_is_still_checked(self):
        source='''module m
implicit none
contains
recursive subroutine visit(i,a)
integer,intent(in)::i
integer,intent(inout)::a(:)
if(i>size(a))return
if(mod(i,2)==1)a(i)=i
call visit(i+1,a)
end subroutine
end module
program main
use m
integer::a(4)
call visit(1,a)
print *,a
end
'''
        self.failure(source,2)

    def failure(self,source,element):
        with tempfile.TemporaryDirectory(prefix='recursive-array-error-') as tmp:
            directory=Path(tmp)
            path=directory/'main.f90'
            path.write_text(source)
            for command in build_commands(path,directory):
                with self.subTest(command=command):
                    result=subprocess.run(command,text=True,capture_output=True,timeout=30)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn('uninitialized variable',result.stderr)
                    self.assertIn(f'element {element}',result.stderr)


if __name__ == '__main__':
    unittest.main(verbosity=2)

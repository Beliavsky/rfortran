"""Runtime CHARACTER lengths and recursive allocatable scalar results."""
import unittest
import test_deallocate
from pathlib import Path
import subprocess
import tempfile
from test_record_read import COMPILER, ROOT

class ScalarResultExtensions(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_example(self):
        self.check((ROOT/'xalloc_scalar_recursive.f90').read_text(encoding='utf-8'),
                   '720 6 xxxxxx\n')

    def test_runtime_character_padding_truncation_and_empty(self):
        source='''program p
implicit none
integer::n
character(:),allocatable::x
read *,n
x=f(n)
print *,len(x),'['//x//']'
print *,len(f(n+1))
contains
function f(n) result(s)
integer,intent(in)::n
character(n),allocatable::s
allocate(s)
s='xy'
end function
end program
'''
        for n,expected in [(4,'4 [xy  ]\n5\n'),(1,'1 [x]\n2\n'),
                           (0,'0 []\n1\n'),(-2,'0 []\n0\n')]:
            with self.subTest(n=n):
                self.check(source,expected,data=f'{n}\n')

    def test_runtime_character_assignment_and_repeat(self):
        self.check('''program p
implicit none
integer::n
character(:),allocatable::x
read *,n
x=f(n)
print *,len(x),x
contains
function f(n) result(s)
integer,intent(in)::n
character(n),allocatable::s
s=repeat('x',n)
end function
end program
''','5 xxxxx\n',data='5\n')

    def test_runtime_character_length_inquiry_evaluates_actual_once(self):
        self.check('''module m
implicit none
integer::count=0
contains
integer function next()
count=count+1
next=count
end function
function text(n) result(s)
integer,intent(in)::n
character(2*n),allocatable::s
s='x'
end function
end module
program p
use m
implicit none
block
integer::length
length=len(text(next()))
print *,length
end block
print *,count
end program
''', '2\n1\n')

    def test_recursive_integer_result(self):
        self.check('''program p
implicit none
integer,allocatable::x
x=fact(6)
print *,x
contains
recursive function fact(n) result(y)
integer,intent(in)::n
integer,allocatable::y
if(n<=1) then
y=1
return
end if
y=n*fact(n-1)
end function
end program
''', '720\n')

    def test_recursive_complex_result(self):
        self.check('''program p
implicit none
complex,allocatable::x
x=power(4)
print *,x
contains
recursive function power(n) result(y)
integer,intent(in)::n
complex,allocatable::y
if(n==0) then
y=(1.0,0.0)
else
y=(1.0,1.0)*power(n-1)
end if
end function
end program
''', '(-4,0)\n')

    def test_recursive_runtime_character(self):
        self.check('''program p
implicit none
character(:),allocatable::x
x=f(3)
print *,len(x),'['//x//']'
contains
recursive function f(n) result(y)
integer,intent(in)::n
character(n),allocatable::y
if(n==0) then
y=''
else
y=f(n-1)//'x'
end if
end function
end program
''', '3 [xxx]\n')

    def test_recursive_fixed_character(self):
        self.check('''program p
implicit none
print '(a)',f(3)
contains
recursive function f(n) result(y)
integer,intent(in)::n
character(5),allocatable::y
if(n==0) then
y=''
else
y='x'//f(n-1)
end if
end function
end program
''', 'xxx  \n')

    def test_mutually_recursive_logical_results(self):
        self.check('''program p
implicit none
print *,even(12),odd(12),even(11),odd(11)
contains
recursive function even(n) result(y)
integer,intent(in)::n
logical,allocatable::y
if(n==0) then
y=.true.
else
y=odd(n-1)
end if
end function
recursive function odd(n) result(y)
integer,intent(in)::n
logical,allocatable::y
if(n==0) then
y=.false.
else
y=even(n-1)
end if
end function
end program
''', 'T F F T\n')

    def test_recursive_result_kinds(self):
        for kind,zero,one,expected in [
            ('integer(1)','0_1','1_1','5'),('integer(2)','0_2','1_2','5'),
            ('integer(8)','0_8','1_8','5'),('real','0.0','1.0','5'),
            ('real(8)','0.0_8','1.0_8','5'),
            ('complex(8)','(0.0_8,0.0_8)','(1.0_8,1.0_8)','(5,5)')]:
            with self.subTest(kind=kind):
                self.check(f'''program p
implicit none
print *,f(5)
contains
recursive function f(n) result(y)
integer,intent(in)::n
{kind},allocatable::y
if(n==0) then
y={zero}
else
y=f(n-1)+{one}
end if
end function
end program
''', expected+'\n')

    def test_unallocated_recursive_result(self):
        self.check('''program p
implicit none
print *,f(2)
contains
recursive function f(n) result(y)
integer,intent(in)::n
integer,allocatable::y
if(n>0) y=f(n-1)
end function
end program
''', error='unallocated result of function')

    def test_uninitialized_recursive_result(self):
        self.check('''program p
implicit none
print *,f(2)
contains
recursive function f(n) result(y)
integer,intent(in)::n
integer,allocatable::y
allocate(y)
if(n>0) y=f(n-1)
end function
end program
''', error='uninitialized')

    def test_pure_recursion(self):
        self.check('''program p
implicit none
print *,f(40)
contains
pure recursive function f(n) result(y)
integer,intent(in)::n
integer,allocatable::y
if(n==0) then
y=0
else
y=f(n-1)+1
end if
end function
end program
''', '40\n')

    def test_runtime_character_value_bound(self):
        self.check('''program p
implicit none
integer::n
read *,n
print *,f(n)
contains
function f(n) result(s)
integer,intent(in)::n
character(n),allocatable::s
s='x'
end function
end program
''', error='exceeds its 4096-character expression buffer', data='4097\n')

    def test_runtime_repeat(self):
        self.check('''program p
implicit none
integer::n
read *,n
print *,repeat(ncopies=n,string='ab')
print *,repeat('',1000000000+n)
end program
''', 'ababab\n\n', data='3\n')

    def test_runtime_repeat_invalid_counts(self):
        source='''program p
implicit none
integer::n
read *,n
print *,repeat('ab',n)
end program
'''
        self.check(source, error='NCOPIES must be nonnegative', data='-1\n')
        self.check(source, error='too large', data='2147483647\n')

    def test_two_recursive_calls_and_repeated_results(self):
        self.check('''program p
implicit none
integer,allocatable::x
x=fib(8)+fib(7)
print *,x,fib(6)
contains
recursive function fib(n) result(y)
integer,intent(in)::n
integer,allocatable::y
if(n<2) then
y=n
else
y=fib(n-1)+fib(n-2)
end if
end function
end program
''', '34 8\n')

    def test_invalid_direct_recursion_without_result_clause(self):
        self.reject('''program p
implicit none
print *,f(3)
contains
recursive function f(n)
integer,intent(in)::n
integer,allocatable::f
if(n==0) then
f=0
else
f=f(n-1)+1
end if
end function
end program
''', 'direct recursion requires an explicit RESULT variable')

    def test_separately_compiled_module(self):
        module='''module scalar_result_module
implicit none
contains
recursive function fact(n) result(y)
integer,intent(in)::n
integer,allocatable::y
if(n<=1) then
y=1
else
y=n*fact(n-1)
end if
end function
function text(n) result(s)
integer,intent(in)::n
character(n),allocatable::s
s=repeat('x',n)
end function
end module
'''
        main='''program p
use scalar_result_module
implicit none
integer::n
character(:),allocatable::s
read *,n
s=text(n)
print *,fact(7),len(s),s
end program
'''
        self.check(module+main,'5040 4 xxxx\n',data='4\n')
        with tempfile.TemporaryDirectory(prefix='scalar-result-extensions-') as tmp:
            directory=Path(tmp)
            (directory/'m.f90').write_text(module,encoding='utf-8')
            (directory/'main.f90').write_text(main,encoding='utf-8')
            for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                            [COMPILER,'main.f90','m.obj','-o','main.exe']):
                result=subprocess.run(command,cwd=directory,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
            result=subprocess.run([directory/'main.exe'],input='4\n',capture_output=True,text=True,timeout=15)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.split(),['5040','4','xxxx'])

    def test_recursive_array_input(self):
        self.check('''program p
implicit none
real::a(3)
a=[1.0,2.0,3.0]
print *,total(a,size(a))
contains
pure recursive function total(a,n) result(y)
real,intent(in)::a(:)
integer,intent(in)::n
real,allocatable::y
if(n==0) then
y=0.0
else
y=a(n)+total(a,n-1)
end if
end function
end program
''', '6\n')

if __name__=='__main__':
    unittest.main(verbosity=2)

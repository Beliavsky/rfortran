"""Module host/USE association and shared mutable storage."""
import unittest
import test_deallocate
from test_record_read import ROOT
class ModuleScope(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_character_array_constant_reproducer(self):
        self.check((ROOT/'tests/174_module_scope.f90').read_text(encoding='utf-8'),'2\n')

    def test_shared_scalar_and_caller_shadowing(self):
        self.check("""module counter_mod
implicit none
integer::counter=10
contains
pure integer function get_counter() result(n)
n=counter
end function
subroutine increment()
counter=counter+1
end subroutine
end module
program main
use counter_mod,only:get_counter,increment
implicit none
integer::counter
counter=-7
print *,get_counter(),counter
call increment()
print *,get_counter(),counter
end program
""",'10 -7\n11 -7\n')

    def test_other_modules_and_reexports(self):
        self.check("""module base
implicit none
integer,parameter::factor=3
integer::offset=4
contains
pure integer function times_factor(n) result(j)
integer,intent(in)::n
j=n*factor
end function
end module
module middle
use base,only:factor,offset,times_factor
implicit none
contains
pure integer function evaluate(n) result(j)
integer,intent(in)::n
j=times_factor(n)+offset+factor
end function
end module
program main
use middle,only:evaluate,offset
implicit none
print *,evaluate(2)
offset=8
print *,evaluate(2)
end program
""",'13\n17\n')

    def test_mutable_numeric_array(self):
        self.check("""module arrays
implicit none
integer::a(3)=[1,2,3]
contains
subroutine change()
a(2)=20
end subroutine
pure integer function total() result(j)
j=sum(a)
end function
end module
program main
use arrays
implicit none
print *,total()
call change()
print *,a,total()
a(1)=10
print *,total()
end program
""",'6\n1 20 3 24\n33\n')

    def test_mutable_character_scalar_and_array(self):
        self.check("""module labels
implicit none
character(4)::label='red'
character(4)::names(2)=['one ','two ']
contains
function get_label() result(text)
character(4)::text
text=label
end function
function get_name(i) result(text)
integer,intent(in)::i
character(4)::text
text=names(i)
end function
subroutine change()
label='blue'
names(2)='last'
end subroutine
end module
program main
use labels
implicit none
print '(a)',trim(get_label())
print '(a)',trim(get_name(2))
call change()
print '(a)',trim(get_label())
print '(a)',trim(get_name(2))
end program
""",'red\ntwo\nblue\nlast\n')

    def test_same_named_procedures_in_different_modules(self):
        self.check("""module first
contains
integer function f() result(n)
n=1
end function
end module
module second
contains
integer function f() result(n)
n=2
end function
end module
program main
implicit none
print *,one(),two()
contains
integer function one() result(n)
use first,only:f
n=f()
end function
integer function two() result(n)
use second,only:f
n=f()
end function
end program
""",'1 2\n')

    def test_use_renames(self):
        self.check("""module m
implicit none
integer,parameter::step=2
integer::value=5
contains
integer function getter() result(j)
j=value
end function
end module
program main
use m,only:n=>step,x=>value,read_x=>getter
implicit none
print *,n,read_x()
x=9
print *,read_x()
end program
""",'2 5\n9\n')
        self.check("""module m
integer::value=5
integer,parameter::n=2
end module
use m,x=>value
implicit none
x=7
print *,x,n
end
""",'7 2\n')

    def test_rank_two_array_and_scalar_kinds(self):
        self.check("""module m
use iso_fortran_env,only:int64,real64
implicit none
integer(int64)::a(2,2)
real(real64)::weight=0.5_real64
logical::enabled=.true.
contains
subroutine initialize()
a=reshape([1_int64,2_int64,3_int64,4_int64],[2,2])
end subroutine
pure integer(int64) function total() result(j)
j=sum(a)
end function
end module
use m
implicit none
call initialize()
print *,total(),a(2,2),weight,enabled
end
""",'10 4 0.5 T\n')

    def test_module_allocatable_status_and_values(self):
        self.check("""module m
implicit none
integer,allocatable::a(:)
contains
subroutine fill()
allocate(a(3))
a=[1,2,3]
end subroutine
pure integer function total() result(j)
j=sum(a)
end function
subroutine clear()
deallocate(a)
end subroutine
end module
program main
use m
implicit none
print *,allocated(a)
call fill()
print *,allocated(a),size(a),total()
call clear()
print *,allocated(a)
end program
""",'F\nT 3 6\nF\n')

    def test_keyword_module_call_and_local_constant_shadowing(self):
        self.check("""module m
implicit none
integer,parameter::step=5
contains
pure integer function add(a,b) result(j)
integer,intent(in)::a,b
j=a+b+step
end function
end module
program main
use m,only:add
implicit none
integer,parameter::step=99
print *,add(b=2,a=1),step
end program
""",'8 99\n')

    def test_module_array_partial_initialization(self):
        self.check("""module m
integer::a(2)
contains
subroutine fill()
a(2)=7
end subroutine
integer function read_second() result(j)
j=a(2)
end function
end module
use m
call fill()
print *,read_second()
end
""",'7\n')
        self.reject("""module m
integer::a(2)
contains
subroutine fill()
a(2)=7
end subroutine
integer function read_first() result(j)
j=a(1)
end function
end module
use m
call fill()
print *,read_first()
end
""",'uninitialized variable')

    def test_generic_alias_reexport_and_extension(self):
        self.check("""module base
interface g
module procedure integer_value
end interface
contains
integer function integer_value(n) result(j)
integer,intent(in)::n
j=n
end function
end module
module extension
use base,only:pick=>g
interface pick
module procedure real_value
end interface
contains
integer function real_value(n) result(j)
real,intent(in)::n
j=int(n)+1
end function
end module
use extension,only:pick
implicit none
print *,pick(2),pick(3.0)
end
""",'2 4\n')

    def test_multiple_aliases_of_one_entity(self):
        self.check("""module m
integer::value=3
end module
use m,only:value,x=>value,y=>value
implicit none
x=8
print *,value,y
end
""",'8 8\n')

    def test_component_default_uses_host_constant(self):
        self.check("""module m
integer,parameter::n=7
type :: t
integer::n=n
end type t
end module
use m
implicit none
type(t)::x
print *,x%n
end
""",'7\n')

    def test_shared_allocatable_matrix(self):
        self.check("""module m
integer,allocatable::a(:,:)
contains
subroutine fill()
allocate(a(2,3))
a=4
end subroutine
integer function total() result(n)
n=sum(a)
end function
end module
use m
implicit none
call fill()
print *,size(a,1),size(a,2),total()
deallocate(a)
print *,allocated(a)
end
""",'2 3 24\nF\n')

    def test_runtime_uninitialized_shared_storage(self):
        self.reject("""module m
integer::value
contains
integer function read_value() result(j)
j=value
end function
end module
use m
print *,read_value()
end
""",'uninitialized variable')

    def test_private_only_and_pure_writes_remain_rejected(self):
        self.reject("""module m
private
integer::hidden=1
end module
use m,only:hidden
end
""",'not a public entity')
        self.reject("""module m
integer::value=1
contains
pure subroutine bad()
value=2
end subroutine
end module
end
""",'nonlocal variable')

if __name__=='__main__':unittest.main()

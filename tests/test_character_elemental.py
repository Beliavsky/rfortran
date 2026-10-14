"""General elemental scalar results, including caller-sized CHARACTER buffers."""
import unittest
import test_deallocate
from test_record_read import ROOT

class CharacterElemental(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_original_example(self):
        self.check((ROOT/'tests/143_character_elemental.f90').read_text(),
                   "        ab@        cd@        ef@\n")

    def test_scalar_arrays_aliases_and_early_return(self):
        self.check("""module strings
contains
elemental function join(a,b) result(c)
character(len=*),intent(in)::a,b
character(len=len(a)+len(b))::c
if (len_trim(a)==0) then
c=b
return
end if
c=trim(a)//trim(b)
end function join
end module strings
program main
use strings
character(len=2)::a(3)
character(len=4)::b(3)
a=['a ','  ','c ']
b=join(a,'!')
print '(3(a,1x))',b
print '(a)',join('  ','ok')
a=join(a,'?')
print '(3(a,1x))',a
b=join(join(a,'@'),'#')
print '(3(a,1x))',b
end program main
""", "a!   !    c!   \nok  \na? ?  c? \na?@# ?@#  c?@# \n")

    def test_logical_and_numeric_results_with_character_inputs(self):
        self.check("""program main
logical::mask(3)
integer::n(3)
mask=same_text(['ab','cd','ab'],'ab')
n=length_text(['ab','cd','ef'])
print *,mask,n
print *,positive([-1,0,2])
contains
elemental logical function same_text(a,b)
character(len=*),intent(in)::a,b
same_text=a==b
end function same_text
elemental integer function length_text(a)
character(len=*),intent(in)::a
length_text=len_trim(a)
end function length_text
elemental logical function positive(n)
integer,intent(in)::n
positive=n>0
end function positive
end program main
""", "T F T 2 2 2\nF F T\n")

    def test_different_argument_lengths_and_empty_array(self):
        self.check("""program main
character(len=5)::out(2)
character(len=3)::empty(0)
out=join(['a','b'],['xx','yy'])
print '(2(a,1x))',out
print *,len(join('a','xyz'))
print *,size(join([character(len=2)::], '!'))
empty=join([character(len=2)::],'!')
print *,size(empty),len(empty)
contains
elemental function join(a,b) result(c)
character(len=*),intent(in)::a,b
character(len=len(a)+len(b))::c
c=a//b
end function join
end program main
""", "axx   byy   \n4\n0\n0 3\n")

    def test_scalar_actual_evaluated_once_and_fixed_dummy(self):
        self.check("""program main
character(len=2)::out(2)
out=choose(['ab','cd'],next())
print '(2(a,1x))',out
contains
impure integer function next()
print *,'called'
next=1
end function next
elemental character(len=2) function choose(a,n)
character(len=2),intent(in)::a
integer,intent(in)::n
if (n>0) then
choose=a
else
choose='??'
end if
end function choose
end program main
""", "called\nab cd \n")

    def test_sections_substrings_and_fixed_dummy_length(self):
        self.check("""program main
character(len=3)::a(3)
a=['abc','def','ghi']
print '(2(a,1x))',first_two(a(3:1:-2))
print '(a)',first_two(a(1)(2:3))
contains
elemental function first_two(a) result(b)
character(len=2),intent(in)::a
character(len=len(a))::b
b=a
end function first_two
end program main
""", "gh ab \nbc\n")

    def test_condition_recomputed_and_impure_elemental(self):
        self.check("""program main
integer::i
character(len=1)::a(3),b(2)
a=['a','b',' ']
i=1
do while(nonblank(a(i)))
i=i+1
end do
print *,i
b=traced(a(:2))
print '(2a)',b
contains
elemental logical function nonblank(a)
character(len=*),intent(in)::a
nonblank=len_trim(a)>0
end function nonblank
impure elemental function traced(a) result(b)
character(len=*),intent(in)::a
character(len=len(a))::b
print '(a)',a
b=a
end function traced
end program main
""", "3\na\nb\nab\n")

    def test_mismatched_shapes_and_types(self):
        routine="""contains
elemental function join(a,b) result(c)
character(len=*),intent(in)::a,b
character(len=len(a)+len(b))::c
c=a//b
end function join
end program main"""
        self.reject("program main\nprint *,join(['a','b'],['x','y','z'])\n"+routine,"nonconforming")
        self.reject("program main\nprint *,join(1,'a')\n"+routine,"CHARACTER actual")

    def test_named_arguments_and_formatted_write(self):
        self.check("""program main
write(*,'(a)',advance='no')copy(a='ab')
write(*,'(a)')copy(a='cd')
write(*,'(2a)')copy(['x','y'])
contains
elemental function copy(a) result(b)
character(len=*),intent(in)::a
character(len=len(a))::b
b=a
end function copy
end program main
""", "abcd\nxy\n")

    def test_unsupported_contexts_are_diagnosed(self):
        routine="""contains
elemental integer function width(a)
character(len=*),intent(in)::a
width=len_trim(a)
end function width
end program main"""
        self.reject("program main\ninteger::x(2)\ncharacter(len=1)::text(2)\nx=1\ntext=['a','b']\nwhere(x>0)x=width(text)\n"+routine,
                    "inside WHERE")
        self.reject("program main\ncharacter(len=2)::text\ntext='a'\nprint *,width(trim(text))\n"+routine,
                    "runtime TRIM actuals")
        self.reject("""module data
integer::host(1)
contains
impure elemental function copy(a) result(b)
character(len=*),intent(in)::a
character(len=len(a))::b
if(host(1)>0)then
b=a
else
b='?'
end if
end function copy
end module data
program main
use data
print *,copy('a')
end program main
""", "uninitialized")
        # Module host association is supported; initialized host storage must
        # also be visible to the expanded CHARACTER procedure.
        self.check("""module data
integer::host(1)=1
contains
impure elemental function copy(a) result(b)
character(len=*),intent(in)::a
character(len=len(a))::b
if(host(1)>0)then
b=a
else
b='?'
end if
end function copy
end module data
program main
use data
print *,copy('a')
end program main
""", "a\n")

    def test_uninitialized_input_and_purity(self):
        self.reject("""program main
character(len=2)::text
print *,copy(text)
contains
elemental function copy(a) result(b)
character(len=*),intent(in)::a
character(len=len(a))::b
b=a
end function copy
end program main
""", "uninitialized variable")
        self.reject("""program main
print *,copy('ab')
contains
elemental function copy(a) result(b)
character(len=*),intent(in)::a
character(len=len(a))::b
print *,a
b=a
end function copy
end program main
""", "external I/O")

if __name__=='__main__':
    unittest.main()

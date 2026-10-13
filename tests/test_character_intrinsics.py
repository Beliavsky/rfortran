"""CHARACTER searches and transformations in both execution paths and gfortran."""
import unittest
import test_deallocate


class CharacterIntrinsics(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_runtime_search_direction_and_empty_patterns(self):
        self.check("""character(9)::text
logical::back
integer::i
text='abc123abc'
do i=1,2
back=i==2
print *,scan(text,'abc',back),verify(text,'abc',back),index(text,'abc',back)
print *,index(text,'',back),scan(text,'',back),verify(text,'',back)
end do
print *,index('',''),index('','x'),scan('','x'),verify('','x')
print *,scan(text,' '),verify('   ',' ')
end
""", '1 4 1\n1 0 1\n9 6 7\n10 0 9\n1 0 0 0\n0 0\n')

    def test_keywords_kinds_and_parameter_initializers(self):
        self.check("""integer(8),parameter::p=scan(set='123',string='abc123',kind=8)
integer,parameter::q=verify('abc123','abc',back=.true.)
integer,parameter::r=index('abcabc','abc',back=.true.)
character(6)::text
text='abcabc'
print *,p,q,r
print *,index(substring='abc',string=text,back=.true.,kind=8)
print *,kind(scan(text,'a',kind=8)),kind(verify(text,'b',kind=2)),kind(index(text,'c',kind=1))
print *,scan(text,'a',kind=8),verify(text,'b',kind=2),index(text,'c',kind=1)
end
""", '4 6 4\n4\n8 2 1\n1 1 3\n')

    def test_elemental_arrays_and_shape_conformance(self):
        self.check("""character(4)::words(2)
words=['ab12','9cab']
print *,scan(words,'ab')
print *,verify(string=words,set='ab')
print *,index('abcabc',substring=['ab','bc'],back=.true.)
print *,scan(words,['ab','ac'],back=.true.)
print *,len_trim(['abc ',' abc'])
print *,iachar(['a','z'])
end
""", '1 3\n3 1\n4 5\n2 3\n3 4\n97 122\n')
        self.reject("print *,scan(['a','b'],['a','b','c'])\nend\n", 'nonconforming')

    def test_adjustment_assignments_concatenations_and_arrays(self):
        self.check("""character(4)::text,left,right
text=' ab '
left=adjustl(string=text)
right=adjustr(text)
print *, '['//left//']['//right//']'
print *, '['//adjustl('  ')//']['//adjustr('')//']'
print *,len_trim(adjustl(text)),len_trim(adjustr(text))
print *, '['//adjustl([' ab ',' c  '])//']'
print *, '['//adjustr([' ab ',' c  '])//']'
end
""", '[ab  ][  ab]\n[  ][]\n2 4\n[ab  ][c   ]\n[  ab][   c]\n')

    def test_character_star_length_and_empty_formatted_print(self):
        self.check("""character*5::text='hi'
character*(3)::other
other='abcdef'
print *,len(text),'['//text//']',len(other),other
print "('hello')"
print "(a)",'a'//new_line('x')//'b'
print "('bye')"
end
""", '5 [hi   ] 3 abc\nhello\na\nb\nbye\n')

    def test_format_colon_and_escaped_quotes(self):
        self.check("""character(30)::fmt
fmt="(a,:,'unexpected',a)"
print fmt,'abc'
print "(*(:,'''',a,''' '))",adjustl(' abc'),adjustr('abc ')
print "('before',:,'unexpected')"
end
""", "abc\n'abc ' ' abc'\nbefore\n")

    def test_empty_print_preserves_print_as_a_binding(self):
        self.check("""module pairs
type :: pair
integer::n
contains
procedure::print=>emit
end type pair
contains
subroutine emit(this)
class(pair),intent(in)::this
print *,this%n
end subroutine emit
end module pairs
use pairs
type(pair)::p
p%n=7
call p%print()
if(.true.)print "('formatted')"
end
""",'7\nformatted\n')

    def test_pure_search_procedure(self):
        self.check("""print *,last_digit('a1b2')
contains
pure integer function last_digit(text)
character(*),intent(in)::text
last_digit=scan(text,'0123456789',back=.true.)
end function last_digit
end
""", '4\n')

    def test_back_function_is_evaluated_once(self):
        self.check("""integer::n
n=index('abcabc','abc',back=direction())
print *,n
n=scan('abcabc','abc',back=direction())
print *,n
print *,verify('abc123','abc',back=all([.true.,.true.]))
contains
logical function direction()
integer,save::calls=0
calls=calls+1
direction=calls==1
end function direction
end
""", '4\n1\n6\n')

    def test_invalid_search_arguments(self):
        for expression, message in [
            ("scan(1,'x')", 'CHARACTER'),
            ("verify('x',1)", 'CHARACTER'),
            ("scan('x','x',back=1)", 'scalar LOGICAL'),
            ("index('x','x',kind=16)", 'supported INTEGER kind'),
            ("scan(string='x',set='x',unknown=.true.)", 'unknown keyword'),
            ("verify('x','x',set='x')", 'duplicate argument'),
        ]:
            with self.subTest(expression=expression):
                self.reject('print *,'+expression+'\nend\n',message)


if __name__ == '__main__':
    unittest.main()

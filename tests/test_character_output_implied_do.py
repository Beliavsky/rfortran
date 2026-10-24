"""Evaluate CHARACTER output expressions inside their implied-DO iterations."""
import unittest

import test_deallocate
from test_record_read import ROOT


REPRO = (ROOT/'tests/programs/xacv_allocatable_character_implied_do.f90').read_text()
WORDS = """implicit none
character(8),allocatable::words(:)
integer::i,n
allocate(words(3))
words=[character(8)::'alpha','beta','gamma']
"""


class CharacterOutputImpliedDo(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_xacv_repro_and_preinitialized_index(self):
        for initial in ('', 'i=1\n', 'i=9\n'):
            with self.subTest(initial=initial):
                self.check(REPRO.replace('nread = size(words)', 'nread = size(words)\n'+initial),
                           'alpha beta\n')

    def test_print_and_formatted_variants(self):
        for output in ('print *,', "write(*,'(*(a))')"):
            with self.subTest(output=output):
                self.check(REPRO.replace('write(*,*)', output), 'alpha beta\n')

    def test_runtime_bounds_reverse_and_empty(self):
        program = WORDS+'''read *,n
write(*,*) 'start ',(trim(words(i))//' ',i=1,n),'end'
write(*,*) 'reverse ',(trim(words(i))//' ',i=n,1,-1),'end'
deallocate(words)
end
'''
        self.check(program, 'start alpha beta gamma end\nreverse gamma beta alpha end\n', data='3\n')
        self.check(program, 'start end\nreverse end\n', data='0\n')

    def test_nested_dependent_controls(self):
        self.check('''implicit none
character(3)::words(2,2)
integer::i,j
words(1,1)='a';words(2,1)='b'
words(1,2)='c';words(2,2)='d'
write(*,*) ((trim(words(j,i))//' ',j=1,i),i=1,2)
end
''', 'a c d\n')

    def test_function_evaluated_once_per_iteration(self):
        program = '''module output_calls_mod
implicit none
integer::calls=0
contains
function word(i) result(text)
integer,intent(in)::i
character(1)::text
calls=calls+1
text=achar(96+i)
end function
end module
program main
use output_calls_mod
implicit none
integer::i,n
n=3
write(*,*) (word(i)//' ',i=1,n)
print *,calls
n=0
write(*,*) (word(i)//' ',i=1,n)
print *,calls
end
'''
        self.check(program, 'a b c\n3\n\n3\n')
        # Adjacent CHARACTER values retain the stdout printer's spacing.
        self.check(program.replace("word(i)//' '", 'word(i)'), 'abc\n3\n\n3\n')

    def test_function_controls_are_evaluated_once(self):
        self.check('''module output_bounds_mod
implicit none
integer::calls=0
contains
integer function limit(n) result(value)
integer,intent(in)::n
calls=calls+1
value=n
end function
end module
program main
use output_bounds_mod
implicit none
character(5)::words(3)=['alpha','beta ','gamma']
integer::i
write(*,*) (trim(words(i))//' ',i=limit(1),limit(3),limit(1))
print *,calls
write(*,*) (trim(words(i))//' ',i=limit(1),limit(0),limit(1))
print *,calls
end
''', 'alpha beta gamma\n3\n\n6\n')

    def test_file_variable_stdout_and_internal_output(self):
        self.check(WORDS.replace('integer::i,n', 'integer::i,n,u\ncharacter(80)::line')+'''n=3
open(newunit=u,status='scratch')
write(u,*) (trim(words(i))//' ',i=1,n)
rewind(u)
read(u,'(a)')line
print '(a)',trim(line)
close(u)
u=6
write(u,*) (trim(words(i))//' ',i=1,n)
open(20,status='scratch')
write(20,*) (trim(words(i))//' ',i=1,n)
rewind(20)
read(20,'(a)')line
print '(a)',trim(line)
close(20)
write(line,*) (trim(words(i))//' ',i=1,n)
print '(a)',trim(line)
deallocate(words)
end
''', 'alpha beta gamma\n'*4)

    def test_unit_value_is_captured_before_items(self):
        self.check('''module output_unit_mod
implicit none
integer::u=20
contains
function word(i) result(text)
integer,intent(in)::i
character(1)::text
u=6
text=achar(96+i)
end function
end module
program main
use output_unit_mod
implicit none
integer::i
character(40)::line
open(20,status='scratch')
write(u,*) (word(i)//' ',i=1,2)
rewind(20)
read(20,'(a)')line
print '(a)',trim(line)
print *,u
close(20)
end
''', 'a b\n6\n')

    def test_function_output_inside_select_case(self):
        self.check("""implicit none
integer::values(1)
values=choose(1)
print *,values
contains
function choose(n) result(values)
integer,intent(in)::n
integer::values(1),i
character(5)::words(2)=['alpha','beta ']
select case(n)
case(1)
   write(*,*) (trim(words(i))//' ',i=1,2)
case default
   print *, (trim(words(i))//' ',i=2,1,-1)
end select
values=n
end function
end
""", 'alpha beta\n1\n')

    def test_uninitialized_element_is_still_checked(self):
        program = '''implicit none
character(8),allocatable::words(:)
integer::i,n
allocate(words(2))
words(1)='alpha'
read *,n
write(*,*) (trim(words(i))//' ',i=1,n)
end
'''
        self.check(program, 'alpha\n', data='1\n')
        self.check(program, error='uninitialized variable', data='2\n', reference=False)

    def test_zero_step_is_checked_before_item_evaluation(self):
        self.check('''implicit none
character(8),allocatable::words(:)
integer::i,step
allocate(words(2))
read *,step
write(*,*) (trim(words(i))//' ',i=1,2,step)
end
''', error='step must not be zero', data='0\n', reference=False)


if __name__ == '__main__':
    unittest.main()

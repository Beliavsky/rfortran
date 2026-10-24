"""Runtime-sized CHARACTER allocatable outputs share the caller's descriptor."""
import unittest

import test_deallocate
from test_record_read import ROOT


class CharacterAllocatableDummies(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_xacv_read_words(self):
        self.check((ROOT/'tests/programs/xacv_read_words_allocatable.f90').read_text(),
                   '3\nalpha\nbeta\ngamma\n')

    def test_optional_count_and_empty_input(self):
        original = (ROOT/'tests/programs/xacv_read_words_allocatable.f90').read_text()
        for line, expected in [('0', '0\n'), ('-3', '0\n'),
                               ('2 red blue', '2\nred\nblue\n')]:
            with self.subTest(line=line):
                source = original.replace('3 alpha beta gamma', line)
                source = source.replace('integer::i\n', 'integer::i,nread\n')
                source = source.replace('call read_words_line(10,words)',
                                        'call read_words_line(10,words,nread)')
                source = source.replace('print *,size(words)', 'print *,nread')
                self.check(source, expected)

    def test_output_reallocation_forwarding_and_bounds(self):
        self.check('''implicit none
character(3),allocatable::words(:)
integer::n
read *,n
allocate(words(1))
words='old'
call outer(words,n)
print *,allocated(words),size(words),lbound(words,1),ubound(words,1),len(words)
print '(a)',words
call outer(words,0)
print *,allocated(words),size(words),lbound(words,1),ubound(words,1)
call outer(words,1)
print *,size(words),lbound(words,1),ubound(words,1)
print '(a)',words
contains
subroutine outer(words,n)
character(*),allocatable,intent(out)::words(:)
integer,intent(in)::n
call inner(words,n)
end subroutine
subroutine inner(words,n)
character(*),allocatable,intent(out)::words(:)
integer,intent(in)::n
allocate(words(-2:n-3))
words='new'
end subroutine
end
''', 'T 2 -2 -1 3\nnew\nnew\nT 0 1 0\n1 -2 -2\nnew\n', data='2\n')

    def test_inout_and_unallocated_return(self):
        self.check('''implicit none
character(4),allocatable::words(:)
call change(words,.true.)
print *,allocated(words),size(words)
print '(a)',words
call change(words,.false.)
print *,allocated(words)
contains
subroutine change(words,keep)
character(*),allocatable,intent(inout)::words(:)
logical,intent(in)::keep
if(allocated(words)) deallocate(words)
if(.not.keep) return
allocate(words(2))
words='test'
end subroutine
end
''', 'T 2\ntest\ntest\nF\n')

    def test_character_lengths(self):
        for width in (0, 1, 7):
            with self.subTest(width=width):
                self.check(f'''implicit none
character({width}),allocatable::words(:)
call make(words,3)
print *,len(words),size(words),len_trim(words(1))
contains
subroutine make(words,n)
character(*),allocatable,intent(out)::words(:)
integer,intent(in)::n
allocate(words(n))
words='abc'
end subroutine
end
''', f'{width} 3 {min(width,3)}\n')

    def test_large_runtime_extent(self):
        self.check('''implicit none
character(2),allocatable::words(:)
integer::n
read *,n
call make(words,n)
print *,len(words),size(words)
print '(a)',words(n)
contains
subroutine make(words,n)
character(*),allocatable,intent(out)::words(:)
integer,intent(in)::n
allocate(words(n))
words='ok'
end subroutine
end
''', '2 4097\nok\n', data='4097\n')

    def test_out_deallocates_even_when_callee_does_not_allocate(self):
        self.check('''implicit none
character(2),allocatable::words(:)
allocate(words(2))
words='ok'
call reset(words)
print *,allocated(words)
contains
subroutine reset(words)
character(*),allocatable,intent(out)::words(:)
end subroutine
end
''', 'F\n')


if __name__ == '__main__':
    unittest.main()

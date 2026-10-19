"""Bounded formatted READ lists, against gfortran and native O0/O2."""
import unittest
import test_record_read_iostat as shared
import test_fortran2023 as recent
from test_enum_types import ENUM


class FormattedReadLists(unittest.TestCase):
    setUp=shared.RecordReadIOStat.setUp
    commands=shared.RecordReadIOStat.commands
    execute=shared.RecordReadIOStat.execute
    check=recent.Fortran2023.check
    reject=recent.Fortran2023.reject

    def test_example(self):
        self.check((shared.ROOT/'tests/programs/xformatted_read_lists.f90').read_text(),
                   ' 60 50 30 40 10 20\n 0\n',data=(shared.ROOT/'tests/io/formatted_read_lists.txt').read_text())

    def test_sections_strides_and_record_reversion(self):
        commands=self.commands('''implicit none
integer::a(-2:4),status
a=-1
read(*,'(2i3)',iostat=status)a(4:-2:-2)
print *,status,a
read(*,'(i3)')a(3:1:-1)
print *,a
end
''',reference=True)
        self.execute(commands,b' 10 20\n 30 40\n  1\n  2\n  3\n','0 40 -1 30 -1 20 -1 10\n40 -1 30 3 2 1 10\n')

    def test_implied_do_nested_dependent_bounds_and_final_indices(self):
        commands=self.commands('''implicit none
integer::a(3,3),i,j,status
a=0
read(*,'(6i2)',iostat=status)((a(i,j),i=1,j),j=1,3)
print *,status,a,i,j
read(*,'(3i2)')(a(i,1),i=3,1,-1)
print *,a(:,1),i
end
''',reference=True)
        self.execute(commands,b' 1 2 3 4 5 6\n 7 8 9\n','0 1 0 0 2 3 0 4 5 6 4 4\n9 8 7 0\n')

    def test_mixed_scalar_array_lists_and_dependent_designators(self):
        commands=self.commands('''implicit none
integer::a(4),i,n,status
a=0
read(*,'(5i2)',iostat=status)n,(a(i),i=1,2),a(n),a(4)
print *,status,n,a,i
end
''',reference=True)
        self.execute(commands,b' 3 1 2 9 8\n','0 3 1 2 9 8 3\n')

    def test_empty_sections_and_zero_trip_do(self):
        commands=self.commands('''implicit none
integer::a(3),i,status
a=-1
i=99
read(*,'(2i2)',iostat=status)a(2:1),(a(i),i=2,1),a(1),a(3)
print *,status,a,i
end
''',reference=True)
        self.execute(commands,b' 4 6\n','0 4 -1 6 2\n')

    def test_mixed_character_real_logical(self):
        commands=self.commands('''implicit none
character(2)::s(3)
real::x(3)
logical::b(3)
integer::i,status
s='--';x=0;b=.false.
read(*,'(2(a2,f4.1,l2))',iostat=status)(s(i),x(i),b(i),i=3,1,-2)
print *,status
print '(3a2)',s
print *,x,b
end
''',reference=True)
        self.execute(commands,b'ab 1.2 Tcd 3.4 F\n','0\ncd--ab\n3.4 0 1.2 F F T\n')

    def test_error_retained_and_following_record_available(self):
        commands=self.commands('''implicit none
integer::a(3),status,i
read(*,'(3i2)',iostat=status)(a(i),i=1,3)
print *,status>0
read(*,'(3i2)',iostat=status)a
print *,status,a
end
''',reference=True)
        self.execute(commands,b' 1xx 3\n 4 5 6\n','T\n0 4 5 6\n')

    def test_enum_sections_and_implied_do(self):
        self.check('implicit none\n'+ENUM+'''type(season)::a(4)
integer::i,status
a=season(0)
read(*,'(2z2)',iostat=status)a(4:1:-2)
read(*,'(2i3)',iostat=status)(a(i),i=1,3,2)
print '(6i4)',status,a,i
end
''','   0 -99   7 123   6   5\n',data='0607\n-99123\n')

    def test_omitted_reverse_bounds_and_multidimensional_section(self):
        commands=self.commands('''implicit none
integer::a(2,3),b(-1:1),status
a=0;b=-1
read(*,'(4i2)',iostat=status)a(:,3:2:-1)
read(*,'(3i2)')b(::-1)
print *,status,a,b
end
''',reference=True)
        self.execute(commands,b' 1 2 3 4\n 5 6 7\n','0 0 0 3 4 1 2 -1 -1 -1\n')

    def test_zero_trip_list_still_consumes_record(self):
        commands=self.commands('''implicit none
integer::a(2),i,n
a=0
read(*,'(i2)')(a(i),i=2,1)
read(*,'(i2)')n
print *,n,i
end
''',reference=True)
        self.execute(commands,b'ignore\n 7\n','7 2\n')

    def test_complex_implied_do(self):
        commands=self.commands('''implicit none
complex::z(2)
integer::i
read(*,'(4f4.1)')(z(i),i=1,2)
print *,real(z),aimag(z),i
end
''',reference=True)
        self.execute(commands,b' 1.0 2.0 3.0 4.0\n','1 3 2 4 3\n')

    def test_invalid_implied_do_and_section(self):
        prefix='implicit none\ninteger::a(3),i\n'
        for target,message in [('(a(i),i=1,3,0)','step must not be zero'),
                               ('((a(i),i=1,2),i=1,2)','reuse active variable'),
                               ('(i,i=1,3)','redefine active'),
                               ('a(1:4)','outside')]:
            with self.subTest(target=target):
                self.reject(prefix+"read(*,'(i2)')"+target+'\nend\n',message)

    def test_fixed_inquiries_in_loop_bounds(self):
        commands=self.commands('''implicit none
integer::a(-1:1),i
read(*,'(3i2)')(a(i),i=lbound(a,1),ubound(a,1))
print *,a,i,size(a)
end
''',reference=True)
        self.execute(commands,b' 4 5 6\n','4 5 6 2 3\n')

    def test_enumeration_ordinal_checks_in_implied_do(self):
        self.check('''implicit none
enumeration type :: colour
enumerator :: red,orange,green
end enumeration type
type(colour)::a(3)
integer::i,status
read(*,'(3i2)',iostat=status)(a(i),i=3,1,-1)
print *,status,int(a),i
read(*,'(3i2)',iostat=status)a(:)
print *,status>0
end
''',' 0 3 2 1 0\n T\n',data=' 1 2 3\n 1 0 3\n')

    def test_runtime_bounds_diagnosed(self):
        self.reject('''implicit none
integer::a(3),i,n
n=3
read(*,'(i2)')(a(i),i=1,n)
end
''','bounds and step must currently be constant')


if __name__=='__main__':
    unittest.main()

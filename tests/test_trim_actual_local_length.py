"""Runtime lengths of TRIM actuals and dependent local CHARACTER storage."""
from pathlib import Path
import unittest
import test_deallocate as helpers
REPRO=(Path(__file__).parent/'programs/xtrim_actual_local_length.f90').read_text()

class TrimActualLocalLength(unittest.TestCase):
    check=helpers.Deallocate.check

    def test_date_parser(self):
        self.check(REPRO,"1\n")

    def test_lengths_and_guarded_short_inputs(self):
        source=REPRO.replace('x=date(0)', 'x=date(len(s)+len(t))')
        source=source.replace("text='2026-10-25'\nx=parse(trim(text))\nprint *,x%day", """text='2026-10-25'
x=parse(trim(text))
print *,x%day
text='abc'
x=parse(trim(text))
print *,x%day
text=''
x=parse(trim(text))
print *,x%day""")
        self.check(source,"1\n6\n0\n")

    def test_character_array_element_actual(self):
        source=REPRO.replace('character(len=20)::text','character(len=20)::text(2)')
        source=source.replace("text='2026-10-25'", "text=['2026-10-25','2000-01-01']")
        source=source.replace('trim(text)', 'trim(text(2))')
        self.check(source,"1\n")

    def test_operand_evaluated_once(self):
        source=REPRO.replace('implicit none\ntype date', 'implicit none\ninteger::calls=0\ntype date')
        source=source.replace('end module', """function text_value() result(s)
character(len=20)::s
calls=calls+1
s='2026-10-25'
end function
end module""")
        source=source.replace('trim(text)', 'trim(text_value())')
        source=source.replace('print *,x%day', 'print *,x%day,calls')
        self.check(source,"1 1\n")

    def test_nested_forwarding(self):
        source=REPRO.replace('end module', """function forward(s) result(x)
character(len=*),intent(in)::s
type(date)::x
x=parse(s)
end function
end module""")
        source=source.replace('x=parse(trim(text))', 'x=forward(trim(text))')
        self.check(source,"1\n")

    def test_local_length_expression(self):
        source=REPRO.replace('character(len=len(s))::t', 'character(len=len(s)+2)::t')
        source=source.replace('x=date(1)', 'x=date(len(t))')
        self.check(source,"12\n")

if __name__=='__main__':
    unittest.main()

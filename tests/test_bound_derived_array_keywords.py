"""Source-level argument association for bound procedures with record arrays."""
from pathlib import Path
import unittest
import test_deallocate as helpers

REPRO = (Path(__file__).parent / 'programs/xbound_derived_array_keywords.f90').read_text()

class BoundDerivedArrayKeywords(unittest.TestCase):
    check = helpers.Deallocate.check
    reject = helpers.Deallocate.reject

    def test_keywords_and_omitted_array(self):
        self.check(REPRO, "25\n13 16\n12\n")

    def test_positional_arguments(self):
        source = REPRO.replace('select(rows=rows,extra=4)', 'select(rows,4)')
        source = source.replace('adjust(extra=1,rows=rows)', 'adjust(rows,1)')
        self.check(source, "25\n13 16\n12\n")

    def test_reordered_keywords_and_sections(self):
        source = REPRO.replace('select(rows=rows,extra=4)', 'select(extra=4,rows=rows(2:2))')
        source = source.replace('adjust(extra=1,rows=rows)', 'adjust(rows=rows(1:1),extra=1)')
        self.check(source, "22\n13 5\n12\n")

    def test_array_element_receiver(self):
        source = REPRO.replace('type(frame)::df,out', 'type(frame)::df(2),out')
        source = source.replace('df%', 'df(2)%')
        self.check(source, "25\n13 16\n12\n")

    def test_all_optional_arguments_absent(self):
        self.check(REPRO.replace('select(extra=2)', 'select()'), "25\n13 16\n10\n")

    def test_forwarded_optional_array(self):
        source = REPRO.replace('end module', """function forward(self,rows) result(out)
class(frame),intent(in)::self
type(date),intent(in),optional::rows(:)
type(frame)::out
out=self%select(extra=2,rows=rows)
end function
end module""")
        source = source.replace('out=df%select(extra=2)', 'out=forward(df)')
        self.check(source, "25\n13 16\n12\n")

    def test_invalid_keyword_diagnostics(self):
        for actual, message in [
            ('unknown=rows', 'unknown keyword `unknown`'),
            ('rows=rows,rows=rows', 'duplicate argument `rows`'),
            ('self=df,rows=rows', 'unknown keyword `self`'),
        ]:
            with self.subTest(actual=actual):
                self.reject(REPRO.replace('rows=rows,extra=4', actual), message)
        self.reject(REPRO.replace('adjust(extra=1,rows=rows)', 'adjust(extra=1)'),
                    'missing required argument `rows`')

if __name__ == '__main__':
    unittest.main()

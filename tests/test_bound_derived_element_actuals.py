"""Derived-array elements passed to scalar dummies of bound procedures."""
from pathlib import Path
import unittest
import test_deallocate as helpers

REPRO = (Path(__file__).parent / "programs/xbound_derived_element_actual.f90").read_text()

class BoundDerivedElementActuals(unittest.TestCase):
    check = helpers.Deallocate.check

    def test_renamed_bindings_and_optional_array(self):
        self.check(REPRO, "12\n15\n2 15\n")

    def test_keywords(self):
        self.check(REPRO.replace('self%row_pos(rows(k))', 'self%row_pos(idx=rows(k))')
                   .replace('df%update(rows(2))', 'df%update(idx=rows(2))'), "12\n15\n2 15\n")

    def test_array_element_receiver(self):
        source = REPRO.replace('type(frame)::df', 'type(frame)::df(2)')
        source = source.replace('select_rows(df,rows)', 'select_rows(df(1),rows)')
        source = source.replace('call df%update(rows(2))',
                                'print *,df(2)%row_pos(rows(1))\ncall df(2)%update(rows(2))')
        self.check(source, "12\n15\n12\n2 15\n")

    def test_subscript_evaluated_once(self):
        source = REPRO.replace('implicit none\ntype date', 'implicit none\ninteger::calls=0\ntype date')
        source = source.replace('integer function locate', 'integer function next_index() result(i)\ncalls=calls+1\ni=2\nend function\ninteger function locate')
        source = source.replace('call df%update(rows(2))', 'print *,df%row_pos(rows(next_index()))\ncall df%update(rows(next_index()))')
        source = source.replace('print *,rows%day', 'print *,rows%day,calls')
        self.check(source, "12\n15\n15\n2 15 2\n")

    def test_multidimensional_allocatable_actual(self):
        source = REPRO.replace('type(date)::rows(2)', 'type(date),allocatable::rows(:,:)')
        source = source.replace('rows=[date(2),date(5)]',
                                'allocate(rows(2,2))\nrows(1,1)=date(2)\nrows(2,1)=date(5)\nrows(1,2)=date(7)\nrows(2,2)=date(9)')
        source = source.replace('call select_rows(df,rows)', 'print *,df%row_pos(rows(1,2))')
        source = source.replace('df%update(rows(2))', 'df%update(rows(2,2))')
        self.check(source, "17\n2 5 7 19\n")

if __name__ == '__main__':
    unittest.main()

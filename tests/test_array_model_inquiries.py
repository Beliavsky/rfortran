"""Type-only numeric inquiries on specialized array views."""
from pathlib import Path
import unittest
import test_deallocate as helpers
SOURCE=(Path(__file__).parent/'programs/xhuge_array_dummy.f90').read_text()
class ArrayModelInquiries(unittest.TestCase):
    check=helpers.Deallocate.check

    def test_real_array_dummy(self):
        self.check(SOURCE,'T T 8 8\nT T 8 8\n')

    def test_default_real(self):
        self.check(SOURCE.replace('kind=8','kind=4').replace('0._8','0._4'),'T T 4 4\nT T 4 4\n')

    def test_integer_kinds(self):
        for kind in (1,2,4,8):
            with self.subTest(kind=kind):
                source=SOURCE.replace('real(kind=8)',f'integer(kind={kind})').replace('huge(0._8)',f'huge(0_{kind})')
                source=source.replace('tiny(a)==tiny(0._8),','').replace(',kind(tiny(a))','')
                self.check(source,f'T {kind}\nT {kind}\n')

    def test_expression_kind(self):
        self.check(SOURCE.replace('huge(a)','huge(a+1._8)').replace('tiny(a)','tiny(a+1._8)'),
                   'T T 8 8\nT T 8 8\n')

    def test_other_model_inquiries(self):
        source=SOURCE.replace('print *,huge(a)==huge(0._8),tiny(a)==tiny(0._8),kind(huge(a)),kind(tiny(a))',
            'print *,epsilon(a)==epsilon(0._8),digits(a),precision(a),range(a)')
        self.check(source,'T 53 15 307\nT 53 15 307\n')

if __name__=='__main__':
    unittest.main()

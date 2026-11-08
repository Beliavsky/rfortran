"""Kind spelling and excess digits use the literal's original text and own kind."""
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_unused_warnings as shared
from test_record_read import COMPILER


class RealLiteralWarnings(unittest.TestCase):
    check=shared.UnusedWarnings.check

    def test_default_wall_and_extra(self):
        source='print *,3.141592653589793\nend\n'
        self.assertEqual(self.check(source),'')
        self.assertEqual(self.check(source,['-Wextra']),'')
        self.assertEqual(self.check(source,['-Werror']),'')
        warning=self.check(source,['-Wall'])
        self.assertEqual(warning.count('[excess-real-digits]'),1,warning)
        self.assertNotIn('[real-literal-kind]',warning)
        self.assertEqual(self.check(source,['-Wall','-Wno-excess-real-digits']),'')

    def test_style_all_default_real_spellings(self):
        warning=self.check('print *,3.14,1.0,2E0,.5,1.\nend\n',['-Wreal-literal-kind'])
        self.assertEqual(warning.count('[real-literal-kind]'),5,warning)
        self.assertIn('example.f90:1:9',warning)
        self.assertIn('default REAL(4)',warning)

    def test_explicit_kind_and_d_exponent(self):
        self.assertEqual(self.check('''use iso_fortran_env,only:real64
implicit none
integer,parameter::wp=real64
print *,3.14_4,3.14_wp,3.14_real64,3.14D0,3.D0,1D0
end
''',['-Wreal-literal-kind','-Wexcess-real-digits']),'')

    def test_digits_leading_trailing_zeros_and_exponents(self):
        warning=self.check('''print *,1.234567,0.000001234567,1234567e-3,1.2345670000
print *,3.140000000,0000.314000,1000000000000000000000000000000.0
end
''',['-Wexcess-real-digits'])
        self.assertEqual(warning.count('[excess-real-digits]'),4,warning)
        self.assertEqual(warning.count('supplies 7 significant decimal digits'),4,warning)
        self.assertIn('REAL(4) guarantees 6',warning)

    def test_exact_binary_values(self):
        source=f'''print *,16777216.0,1.234375,0.100000001490116119384765625
print *,1208925819614629174706176.0,{5**149}.0e-149
print *,9007199254740992.0_8,{5**1074}.0e-1074_8
end
'''
        self.assertEqual(self.check(source,['-Wexcess-real-digits']),'')

    def test_decimal_tail_must_not_be_lost_in_host_rounding(self):
        warning=self.check('''print *,0.100000001490116119384765626
print *,9007199254740993.0_8
end
''',['-Wexcess-real-digits'])
        self.assertEqual(warning.count('[excess-real-digits]'),2,warning)
        self.assertIn('REAL(8) guarantees 15',warning)

    def test_explicit_double_precision_digits(self):
        warning=self.check('''print *,3.141592653589793d0,3.141592653589793_8
end
''',['-Wreal-literal-kind','-Wexcess-real-digits'])
        self.assertEqual(warning.count('[excess-real-digits]'),2,warning)
        self.assertNotIn('[real-literal-kind]',warning)
        self.assertEqual(warning.count('REAL(8) guarantees 15'),2,warning)

    def test_named_kinds_use_association_and_shadowing(self):
        warning=self.check('''module kinds
implicit none
integer,parameter::wp=selected_real_kind(15)
end module
program p
use kinds,only:rk=>wp
implicit none
print *,3.141592653589793_rk
block
 integer,parameter::rk=4
 print *,3.141592653589793_rk
end block
end
''',['-Wexcess-real-digits'])
        self.assertEqual(warning.count('[excess-real-digits]'),2,warning)
        self.assertIn('REAL(8) guarantees 15',warning)
        self.assertIn('REAL(4) guarantees 6',warning)

    def test_each_literal_once_in_declarations_arrays_and_complex(self):
        warning=self.check('''implicit none
real,save::x=3.14,y=1.23456789
print *,x,y,[3.14,1.0],(1.23456789,0.5)
end
''',['-Wreal-literal-kind','-Wexcess-real-digits'])
        self.assertEqual(warning.count('[real-literal-kind]'),6,warning)
        self.assertEqual(warning.count('[excess-real-digits]'),2,warning)

    def test_comments_strings_and_formats_are_not_real_literals(self):
        self.assertEqual(self.check('''! 3.141592653589793
print *,"3.141592653589793"
print 10,1.0_4
10 format(f10.8)
end
''',['-Wreal-literal-kind','-Wexcess-real-digits']),'')

    def test_data_continuations_and_else_if(self):
        warning=self.check('''implicit none
real::x
data x /3.141592653589793/
if(x<3.14) then
 print *,x
else if(x<3.15) then
 print *,x
end if
print *,x, &
 & 3.16
end
''',['-Wreal-literal-kind','-Wexcess-real-digits'])
        self.assertEqual(warning.count('[real-literal-kind]'),4,warning)
        self.assertEqual(warning.count('[excess-real-digits]'),1,warning)

    def test_fixed_form_locations(self):
        with tempfile.TemporaryDirectory(prefix='rfort-real-literal-fixed-') as tmp:
            path=Path(tmp)/'old.f'
            path.write_text('''      program p
      real x
      data x /3.141592653589793/
      print *,x,3.14
      end
''')
            result=subprocess.run([str(COMPILER),'-Wreal-literal-kind','-Wexcess-real-digits',
                'check',str(path)],capture_output=True,text=True,timeout=30)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stderr.count('[real-literal-kind]'),2,result.stderr)
            self.assertEqual(result.stderr.count('[excess-real-digits]'),1,result.stderr)
            self.assertIn('old.f:3:',result.stderr)

    def test_flags_errors_and_source_controls(self):
        source='print *,3.141592653589793\nend\n'
        self.assertEqual(self.check(source,['-Wreal-literal-kind','-Wno-real-literal-kind']),'')
        self.assertIn('[real-literal-kind]',self.check(source,['-Werror=real-literal-kind'],errors=True))
        self.assertIn('[excess-real-digits]',self.check(source,['-Werror=excess-real-digits'],errors=True))
        self.assertIn('[excess-real-digits]',self.check(source,['-Wall','-Werror'],errors=True))
        self.assertEqual(self.check('!#allow(real-literal-kind)\n'+source,['-Wreal-literal-kind']),'')
        self.assertEqual(self.check('!#allow(excess-real-digits)\n'+source,['-Wall']),'')
        self.assertIn('[real-literal-kind]',self.check('!#deny(real-literal-kind)\n'+source,errors=True))

    def test_existing_precision_loss_remains_independent(self):
        warning=self.check('''implicit none
double precision::x
x=3.141592653589793
print *,x
end
''',['-Wall','-Wreal-literal-kind'])
        for category in ('real-literal-kind','excess-real-digits','precision-loss'):
            self.assertEqual(warning.count(f'[{category}]'),1,warning)

    def test_include_locations(self):
        with tempfile.TemporaryDirectory(prefix='rfort-real-literal-') as tmp:
            directory=Path(tmp)
            (directory/'values.inc').write_text('print *,3.141592653589793\n')
            source=directory/'main.f90'
            source.write_text("include 'values.inc'\nend\n")
            result=subprocess.run([str(COMPILER),'-Wexcess-real-digits','check',str(source)],
                capture_output=True,text=True,timeout=30)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stderr.count('[excess-real-digits]'),1,result.stderr)
            self.assertIn('values.inc:1:9',result.stderr)

    def test_interpreter_and_native_execution(self):
        with tempfile.TemporaryDirectory(prefix='rfort-real-literal-run-') as tmp:
            directory=Path(tmp);source=directory/'main.f90'
            source.write_text('print "(f4.1)",3.141592653589793\nend\n')
            for mode in (['run',str(source)], ['-O','0',str(source),'--run','-o',str(directory/'p.exe')],
                         ['-O','2',str(source),'--run','-o',str(directory/'p.exe')]):
                with self.subTest(mode=mode):
                    result=subprocess.run([str(COMPILER),'-Wexcess-real-digits',*mode],
                        capture_output=True,text=True,timeout=60)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stdout,' 3.1\n')
                    self.assertEqual(result.stderr.count('[excess-real-digits]'),1,result.stderr)


if __name__=='__main__':
    unittest.main()

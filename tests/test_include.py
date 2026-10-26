"""Fortran INCLUDE expansion, source forms, search paths and diagnostics."""
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER


class Include(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='rfort-include-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding='utf-8')
        return path

    def command(self, *args, success=True):
        p = subprocess.run([COMPILER, *map(str,args)], cwd=self.root,
                           capture_output=True, text=True, encoding='utf-8', timeout=45)
        if success:
            self.assertEqual(p.returncode,0,p.stdout+p.stderr)
        else:
            self.assertNotEqual(p.returncode,0,p.stdout+p.stderr)
        return p

    def run_modes(self, source, expected, flags=()):
        for mode in ('interpreter','O0','O2'):
            with self.subTest(mode=mode):
                args = ['run',source] if mode=='interpreter' else ['-O',mode[1:],source,'-o','program.exe','--run']
                p = self.command(*flags,*args)
                self.assertEqual(p.stdout.strip(),expected)
                self.assertEqual(p.stderr,'')

    def test_nested_and_repeated_includes(self):
        self.write('parts/outer.inc', 'INCLUDE "inner.inc" ! nested\n')
        self.write('parts/inner.inc', 'integer,parameter::n=7')
        self.write('step.inc','x=x+1\n')
        source=self.write('main.f90', "implicit none\ninclude 'parts/outer.inc'\ninteger::x\nx=n\ninclude 'step.inc'\ninclude \"step.inc\"\nprint '(i0)',x\nend\n")
        self.run_modes(source,'9',('-std=f90',))

    def test_search_paths_and_source_relative_precedence(self):
        self.write('src/local.inc','integer,parameter::a=3\n')
        self.write('local.inc','integer,parameter::a=99\n')
        self.write('include dir/remote.inc','integer,parameter::b=4\n')
        source=self.write('src/main.f90', "implicit none\ninclude 'local.inc'\ninclude 'remote.inc'\nprint '(i0)',a+b\nend\n")
        self.run_modes(source,'7',('-I',self.root/'include dir'))

    def test_fixed_form_inheritance_and_free_override(self):
        self.write('decl.inc','      INTEGER N\n      PARAMETER (N=7)\n')
        source=self.write('main.f', "      PROGRAM MAIN\n      INCLUDE 'decl.inc'\n      PRINT '(I0)', N\n      END\n")
        self.run_modes(source,'7')
        self.write('free.inc','integer,parameter::n=8\n')
        source=self.write('free.f', "include 'free.inc'\nprint '(i0)',n\nend\n")
        self.run_modes(source,'8',('-ffree-form',))

    def test_comments_strings_and_identifiers(self):
        self.write("quote'!.inc",'integer,parameter::n=4\n')
        source=self.write('main.f90', "implicit none\n! include 'missing.inc'\ninclude 'quote''!.inc'\ninteger::include\ninclude = n\nprint '(i0)',include\nend\n")
        self.run_modes(source,'4')
        source=self.write('array_name.f90', "implicit none\ninteger::include(1)\ninclude (1) = 4\nprint '(i0)',include(1)\nend\n")
        self.run_modes(source,'4')
        source=self.write('continued.f90', '''implicit none
character(*),parameter::s="a&
&include 'missing.inc'&
&b"
print '(a)',s
end
''')
        # The text resembling INCLUDE belongs to a continued character literal.
        p=self.command('parse',source)
        self.assertNotIn('not found',p.stderr)

    def test_missing_malformed_and_cycles(self):
        for line,message in [("include 'missing.inc'",'not found'),
                             ('include variable','quoted filename'),
                             ("include 'broken",'unterminated'),
                             ("include ''",'empty'),
                             ("include 'x.inc'; print *,1",'complete source line')]:
            with self.subTest(line=line):
                source=self.write('bad.f90','! heading\n'+line+'\nend\n')
                p=self.command('-c',source,success=False)
                self.assertIn('bad.f90:2',p.stderr)
                self.assertIn(message,p.stderr)
        self.write('a.inc',"include 'b.inc'\n")
        self.write('b.inc',"include 'a.inc'\n")
        source=self.write('cycle.f90',"include 'a.inc'\nend\n")
        p=self.command('-c',source,success=False)
        self.assertIn('INCLUDE cycle',p.stderr)
        self.assertIn('b.inc:1',p.stderr)

    def test_parse_diagnostics_in_include(self):
        self.write('bad.inc','! heading\ninteger :: = 3\n')
        source=self.write('main.f90',"implicit none\ninclude 'bad.inc'\nend\n")
        for args in ([source],['parse',source],['check',source],['run',source],['emit-obj',source,'out.obj']):
            with self.subTest(args=args):
                p=self.command(*args,success=False)
                self.assertIn('bad.inc:2:',p.stderr)

    def test_semantic_and_parent_diagnostics(self):
        self.write('bad.inc','! heading\nprint *,missing\n')
        source=self.write('main.f90',"implicit none\ninclude 'bad.inc'\nend\n")
        for args in ([source],['check',source],['run',source]):
            p=self.command(*args,success=False)
            self.assertIn('bad.inc:2:',p.stderr)
        self.write('good.inc','integer::x\nx=1\n')
        source=self.write('parent.f90',"implicit none\ninclude 'good.inc'\n! third line\nprint *,missing\nend\n")
        p=self.command(source,success=False)
        self.assertIn('parent.f90:4:',p.stderr)

    def test_warning_and_early_error_locations(self):
        self.write('decl.inc','! heading\ninteger::unused\n')
        source=self.write('main.f90',"implicit none\ninclude 'decl.inc'\nend\n")
        p=self.command('-Wunused-variable',source)
        self.assertIn('decl.inc:2:',p.stderr)
        self.write('decl.inc','! heading\nlogical,parameter::x=3\n')
        p=self.command(source,success=False)
        self.assertIn('decl.inc:2:',p.stderr)

    def test_combined_and_separate_modules(self):
        self.write('value.inc','integer,parameter::n=7\n')
        module=self.write('m.f90',"module m\nimplicit none\ninclude 'value.inc'\nend module\n")
        main=self.write('main.f90',"use m\nimplicit none\nprint '(i0)',n\nend\n")
        p=self.command(module,main,'--run')
        self.assertEqual(p.stdout.strip(),'7')
        self.command('-c',module)
        p=self.command(main,'--run')
        self.assertEqual(p.stdout.strip(),'7')
        self.write('bad.inc','! heading\nprint *,missing\n')
        main=self.write('main.f90',"use m\nimplicit none\ninclude 'bad.inc'\nend\n")
        p=self.command(module,main,success=False)
        self.assertIn('bad.inc:2:',p.stderr)

    def test_cpp_conditionals_precede_fortran_include(self):
        self.write('value.inc','integer,parameter::n=7\n')
        source=self.write('main.F90',"implicit none\n#if 0\ninclude 'missing.inc'\n#endif\ninclude 'value.inc'\nprint '(i0)',n\nend\n")
        self.run_modes(source,'7')
        self.write('value.inc','#if 1\ninteger::n\n#endif\n')
        p=self.command('-c',source,success=False)
        self.assertIn('value.inc:1:',p.stderr)
        self.assertIn('unrecognized token',p.stderr)

    def test_runtime_location(self):
        self.write('read.inc','! heading\nprint *,x\n')
        source=self.write('main.f90',"implicit none\nreal::x,r\ncall random_number(r)\nif(r>1.0) x=1.0\ninclude 'read.inc'\nend\n")
        for opt in (0,2):
            with self.subTest(opt=opt):
                p=self.command('-O',opt,source,'--run',success=False)
                self.assertIn('uninitialized',p.stderr)
                self.assertIn('read.inc:2',p.stderr)

    def test_nesting_limit(self):
        for i in range(130):
            self.write(f'{i}.inc',f"include '{i+1}.inc'\n")
        source=self.write('main.f90',"include '0.inc'\nend\n")
        p=self.command('-c',source,success=False)
        self.assertIn('nesting exceeds 128',p.stderr)


if __name__=='__main__':
    unittest.main(verbosity=2)

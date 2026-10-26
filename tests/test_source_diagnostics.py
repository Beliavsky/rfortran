"""Source locations for early declaration/type errors in all driver modes."""
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER

SOURCE="""module m
implicit none
integer, parameter :: dp = kind(1.0d0)
contains
elemental function sgn_real(xx,thresh) result(yy)
! Return the sign.
real(kind=dp), intent(in) :: xx
real(kind=dp), intent(in), pointer :: thresh
real(kind=dp) :: yy
yy=xx
end function sgn_real
end module m
program main
use m
print *,sgn_real(1.0_dp)
end program main
"""

class SourceDiagnostics(unittest.TestCase):
    def invoke(self,args,directory):
        return subprocess.run([COMPILER,*map(str,args)],cwd=directory,capture_output=True,
                              text=True,encoding='utf-8',timeout=30)

    def assert_attribute(self,result,path,line,column):
        self.assertNotEqual(result.returncode,0)
        self.assertIn(f'{path.name}:{line}:{column}',result.stderr)
        self.assertIn('attribute `pointer` on dummy argument `thresh`',result.stderr)
        self.assertIn('ELEMENTAL function `sgn_real`',result.stderr)
        self.assertNotIn('@module$',result.stderr)
        self.assertIn('^^^^^^^',result.stderr)
        self.assertEqual(result.stderr.count('attribute `pointer`'),1,result.stderr)

    def test_build_check_run_and_emit_obj(self):
        with tempfile.TemporaryDirectory(prefix='source-diagnostics-') as tmp:
            directory=Path(tmp);source=directory/'example.f90';source.write_text(SOURCE,encoding='utf-8')
            for args in ([source],['check',source],['run',source],['emit-obj',source,directory/'example.obj']):
                with self.subTest(args=args):
                    self.assert_attribute(self.invoke(args,directory),source,8,28)

    def test_continuation_and_misleading_comments(self):
        text=SOURCE.replace('real(kind=dp), intent(in), pointer :: thresh',
                            'real(kind=dp), intent(in), &\n! pointer in a comment is not the attribute\n  & POINTER :: thresh')
        with tempfile.TemporaryDirectory(prefix='source-diagnostics-') as tmp:
            directory=Path(tmp);source=directory/'continued.f90';source.write_text(text,encoding='utf-8')
            self.assert_attribute(self.invoke([source],directory),source,10,5)

    def test_same_dummy_name_in_another_procedure(self):
        good="""elemental function good(xx,thresh) result(yy)
real(8),intent(in)::xx,thresh
real(8)::yy
yy=xx+thresh
end function good
"""
        text=SOURCE.replace('contains\n','contains\n'+good,1)
        with tempfile.TemporaryDirectory(prefix='source-diagnostics-') as tmp:
            directory=Path(tmp);source=directory/'procedures.f90';source.write_text(text,encoding='utf-8')
            self.assert_attribute(self.invoke([source],directory),source,13,28)

    def test_combined_build_reports_original_file_and_line(self):
        with tempfile.TemporaryDirectory(prefix='source diagnostics ') as tmp:
            directory=Path(tmp)
            first=directory/'first.f90';first.write_text('! Unicode comment: caf? ?\nmodule first\nend module first\n',encoding='utf-8')
            bad=directory/'bad functions.f90';bad.write_text(SOURCE.split('program main')[0],encoding='utf-8')
            main=directory/'main.f90';main.write_text('program main\nend program main\n',encoding='utf-8')
            result=self.invoke([first,bad,main],directory)
            self.assert_attribute(result,bad,8,28)
            self.assertNotIn('f90c-sources',result.stderr)
            self.assertNotIn('first.f90:',result.stderr)

    def test_combined_source_comment_cannot_forge_origin(self):
        with tempfile.TemporaryDirectory(prefix='source-diagnostics-') as tmp:
            directory=Path(tmp);bad=directory/'bad.f90';main=directory/'main.f90'
            bad.write_text('! Source: invented.f90\n'+SOURCE.split('program main')[0],encoding='utf-8')
            main.write_text('program main\nend program main\n',encoding='utf-8')
            result=self.invoke([bad,main],directory)
            self.assert_attribute(result,bad,9,28)
            self.assertNotIn('invented.f90:',result.stderr)

    def test_kind_initializer_and_nested_storage_errors(self):
        cases=[
            ('program main\n! declaration below\nreal(kind=16)::x\nend program main\n',3,'unsupported REAL kind 16'),
            ('program main\ninteger,parameter::n=1/0\nend program main\n',2,'division by zero'),
            ('program main\ninteger::n\nn=2\nblock\ncharacter(len=n)::text\nend block\nend program main\n',5,'runtime-dependent lengths'),
        ]
        with tempfile.TemporaryDirectory(prefix='source-diagnostics-') as tmp:
            directory=Path(tmp);source=directory/'types.f90'
            for text,line,message in cases:
                with self.subTest(message=message):
                    source.write_text(text,encoding='utf-8');result=self.invoke([source],directory)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(f'{source.name}:{line}:',result.stderr)
                    self.assertIn(message,result.stderr)

    def test_array_function_specialization_location(self):
        text=(Path(__file__).resolve().parent.parent/'tests/programs/xassumed_shape_function_call.f90').read_text(encoding='utf-8')
        # Forwarding is supported. Exceed the element limit of an array
        # expression argument to retain a source-bearing failure in the called procedure.
        text=text.replace('logical::tf', 'logical::tf(size(values))').replace('tf=.not. any(i==values)', 'tf=i/=values').replace('if (not_in_vector(ii(i),jj))', 'if (any(not_in_vector(ii(i),abs(jj))))')
        text=text.replace('call check_values([1,2,3],[1,3])', 'integer::input(3),data(1025)\ninput=[1,2,3]\ndata=1\ncall check_values(input,data)')
        with tempfile.TemporaryDirectory(prefix='source-diagnostics-') as tmp:
            directory=Path(tmp);source=directory/'forward.f90';source.write_text(text,encoding='utf-8')
            for args in ([source],['check',source],['run',source],['emit-obj',source,directory/'forward.obj']):
                with self.subTest(args=args):
                    result=self.invoke(args,directory)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn('forward.f90:15:4',result.stderr)
                    self.assertIn('at most 1024 elements',result.stderr)
                    self.assertIn('if (any(not_in_vector(ii(i),abs(jj))))',result.stderr)
                    self.assertNotIn('@deferred_extent',result.stderr)
            module,main=text.split('program main',1)
            first=directory/'first.f90';first.write_text('module first\nend module first\n',encoding='utf-8')
            source.write_text(module,encoding='utf-8')
            driver=directory/'main.f90';driver.write_text('program main'+main,encoding='utf-8')
            result=self.invoke([first,source,driver],directory)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('forward.f90:15:4',result.stderr)
            self.assertNotIn('f90c-sources',result.stderr)

    def test_errors_of_later_passes_have_locations(self):
        """Errors found while loading modules, after parsing and while lowering
        name the statement that caused them."""
        cases=[
            ('program main\nuse ieee_features\nend program\n',2,'intrinsic module `ieee_features`'),
            ('module m\nuse ieee_exceptions\nend module m\nprogram main\nuse m\nend program\n',2,'intrinsic module `ieee_exceptions`'),
            ('program main\ninteger,allocatable::a(:)\na=[3,1,2]\nprint *,maxloc(a,dim=2)\nend program\n',4,'DIM must be between 1 and 1'),
            ("module m\ncharacter(len=:),allocatable::names(:)\nend module m\nprogram main\nuse m\nallocate(character(len=3)::names(2))\nend program\n",6,'requires deferred-length CHARACTER scalars'),
            ("program main\ncharacter(len=8)::text\ntext='a'\ncall show()\ncontains\nsubroutine show()\nprint *,text\nend subroutine\nend program\n",2,'host association for `text`'),
            ('program main\nprint *,f(1)\ncontains\npure integer function f(i)\ninteger,intent(in)::i\ncall g()\nf=i\nend function\nend program\n',6,'PURE procedure cannot call'),
        ]
        with tempfile.TemporaryDirectory(prefix='source-diagnostics-') as tmp:
            directory=Path(tmp)
            for number,(text,line,message) in enumerate(cases):
                with self.subTest(message=message):
                    source=directory/f'case{number}.f90';source.write_text(text,encoding='utf-8')
                    result=self.invoke(['check',source],directory)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)
                    self.assertIn(f'{source.name}:{line}:',result.stderr)

    def test_valid_program_stays_quiet(self):
        with tempfile.TemporaryDirectory(prefix='source-diagnostics-') as tmp:
            directory=Path(tmp);source=directory/'good.f90'
            source.write_text('program main\ninteger::n\nn=2\nprint *,n\nend program main\n',encoding='utf-8')
            result=self.invoke([source],directory)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout,'')
            self.assertEqual(result.stderr,'')

if __name__=='__main__':
    unittest.main()

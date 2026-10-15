"""Source extraction and honest coverage accounting for the util drivers."""
import unittest

from run_util_coverage import Utility, code_only, summarize, compare_execution


SOURCE = '''module util_mod
implicit none
private
public :: apply, &
          generic
interface generic
  module procedure helper, &
                   other
end interface
integer,parameter :: n=2
contains
pure function helper(i) result(j)
integer,intent(in)::i
integer::j
j=i+1
end function helper
function other(i) result(j)
real,intent(in)::i
real::j
j=i+1
end function other
subroutine apply(i)
integer,intent(inout)::i
! other(i) must not count as a dependency
print *, "other(i)!", 'don''t call other(i)'
i=generic(i)
call nested()
contains
subroutine nested()
i=helper(i)
end subroutine
end subroutine apply
integer function unused()
unused=42
end function unused
end module util_mod
'''


class UtilCoverageRunner(unittest.TestCase):
    def test_mask_literals_comments_and_doubled_quotes(self):
        text = "s='don''t!'; call actual(x) ! ignored(y)\ns=\"a\"\"!\"; call next(x)"
        masked = code_only(text)
        self.assertEqual(len(masked), len(text))
        self.assertIn('call actual(x)', masked)
        self.assertIn('call next(x)', masked)
        self.assertNotIn('ignored', masked)
        self.assertNotIn('don', masked)

    def test_inventory_preserves_internal_procedures(self):
        utility = Utility(SOURCE)
        self.assertEqual([p.qualified for p in utility.procedures],
                         ['helper', 'other', 'apply', 'apply.nested', 'unused'])
        self.assertNotIn('nested', utility.top)
        self.assertIn('contains\nsubroutine nested()', utility.top['apply'].source)
        for p in utility.procedures:
            self.assertEqual(SOURCE.splitlines()[p.start-1:p.end], p.source.splitlines())

    def test_closure_and_unchanged_bodies(self):
        utility = Utility(SOURCE)
        selected = utility.closure(['apply'])
        self.assertEqual(selected, {'apply', 'helper', 'other'})
        extracted = utility.extract(selected)
        self.assertNotIn('public ::', extracted)
        self.assertNotIn('private', extracted)
        self.assertNotIn('function unused', extracted)
        for name in selected:
            self.assertIn(utility.top[name].source, extracted)
        self.assertEqual(set(Utility(extracted).top), selected)

    def test_comment_and_string_calls_do_not_add_dependencies(self):
        utility = Utility(SOURCE)
        selected = utility.closure([], "print *, 'unused()' ! call apply(1)\nprint *, helper(1)")
        self.assertEqual(selected, {'helper'})
        source = utility.extract(selected)
        self.assertIn('module procedure helper\n', source)
        self.assertNotIn('module procedure helper,other', source)

    def test_operator_dependencies(self):
        source = SOURCE.replace('interface generic', 'interface operator(.plus.)', 1)
        utility = Utility(source)
        self.assertEqual(utility.closure([], 'print *, 1 .plus. 2'), {'helper', 'other'})

    def test_dependency_inclusion_is_not_test_coverage(self):
        utility = Utility(SOURCE)
        cases = [{'id':'apply', 'procedures':['apply'], 'included':['apply','helper','other'],
                  'results':{'isolated-O0':{'outcome':'pass'}}}]
        report = summarize(utility, cases)
        self.assertEqual(report['procedure_counts'], {'untested':4, 'pass':1})
        helper = next(p for p in report['procedures'] if p['name']=='helper')
        self.assertEqual(helper['included_by'], ['apply'])
        self.assertEqual(helper['tested_by'], [])

    def test_failures_and_not_run_are_not_passes(self):
        utility = Utility(SOURCE)
        case = {'id':'test', 'procedures':['helper'], 'included':['helper'], 'results':{}}
        self.assertEqual(summarize(utility,[case])['procedures'][0]['status'], 'not_run')
        case['results'] = {'isolated-O0':{'outcome':'pass'},
                           'isolated-O2':{'outcome':'output_mismatch'}}
        self.assertEqual(summarize(utility,[case])['procedures'][0]['status'], 'partial')
        case['results'] = {'isolated-O0': {'outcome':'compile_error'}}
        self.assertEqual(summarize(utility,[case])['procedures'][0]['status'], 'blocked')

    def test_mismatched_end_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'mismatched procedure end'):
            Utility(SOURCE.replace('end function helper', 'end subroutine helper'))

    def test_output_comparison_does_not_hide_failures(self):
        reference = {'run': {'stdout': '2 5 T\n'}}
        actual = {'outcome': 'completed', 'run': {'stdout': ' 2 5 .TRUE.\n'}}
        self.assertEqual(compare_execution(actual, reference), 'pass')
        actual['run']['stdout'] = '2 0 T\n'
        self.assertEqual(compare_execution(actual, reference), 'output_mismatch')
        actual['outcome'] = 'run_crash'
        self.assertEqual(compare_execution(actual, reference), 'run_crash')


if __name__ == '__main__':
    unittest.main(verbosity=2)

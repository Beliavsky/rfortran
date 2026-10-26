"""Executable paths should not obscure real DataFrame output comparisons."""
import unittest

from run_dataframe_compat import comparison_output, equivalent


class DataFrameComparison(unittest.TestCase):
    def test_executable_paths_and_numeric_formatting(self):
        for root in (r'C:\test runs', '/tmp/test runs'):
            with self.subTest(root=root):
                left_path = root + '/rfort/program.exe'
                right_path = root + '/gfortran/program.exe'
                left = dict(command=[left_path], stdout=f' 3.0\n executable: {left_path}\n')
                right = dict(command=[right_path], stdout=f' 3.0000000000000000\n executable: {right_path}\n')
                original = left['stdout']
                self.assertTrue(equivalent(comparison_output(left), comparison_output(right)))
                self.assertEqual(left['stdout'], original)

    def test_real_differences_remain_visible(self):
        left = dict(command=['/run/rfort/program'], stdout='/run/rfort/program\nresult: 3\n')
        right = dict(command=['/run/gfortran/program'], stdout='/run/gfortran/program\nresult: 4\n')
        self.assertFalse(equivalent(comparison_output(left), comparison_output(right)))

    def test_other_paths_are_not_normalized(self):
        left = dict(command=['/run/rfort/program'], stdout='file: /run/rfort/data.csv\n')
        right = dict(command=['/run/gfortran/program'], stdout='file: /run/gfortran/data.csv\n')
        self.assertFalse(equivalent(comparison_output(left), comparison_output(right)))


if __name__ == '__main__':
    unittest.main()

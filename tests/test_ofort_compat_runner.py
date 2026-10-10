"""Keep the compatibility comparator from hiding integer/logical failures."""
import unittest
from run_ofort_compat import equivalent, failure

class CompatibilityReporting(unittest.TestCase):
    def test_numeric_and_logical_comparison(self):
        self.assertTrue(equivalent("1 1.25 .TRUE.\n.FALSE.", " 1 1.2500000 T F"))
        self.assertTrue(equivalent("2.5D+00", "2.5000000E+00"))
        self.assertFalse(equivalent("9007199254740992", "9007199254740993"))
        self.assertFalse(equivalent("T", ".FALSE."))
        self.assertFalse(equivalent("1 2", "1"))
        self.assertFalse(equivalent("1.1", "1.2"))
        self.assertFalse(equivalent("NaN", "1.0"))

    def test_failures_are_not_output_mismatches(self):
        self.assertEqual(failure({"returncode":101},"compile"),"compile_crash")
        self.assertEqual(failure({"returncode":-1073741819},"run"),"run_crash")
        self.assertEqual(failure({"returncode":1},"compile"),"compile_error")
        self.assertEqual(failure({"timeout":True},"run"),"run_timeout")
        self.assertEqual(failure({"launch_error":True},"run"),"run_launch_error")
        self.assertIsNone(failure({"returncode":0},"compile"))

if __name__=="__main__":
    unittest.main()

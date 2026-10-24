"""Compiler timing diagnostics are opt-in and do not change normal output."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER


class CompileProfile(unittest.TestCase):
    def compile(self, text, enabled):
        with tempfile.TemporaryDirectory(prefix='rfort-profile-') as temporary:
            directory = Path(temporary)
            source = directory / 'example.f90'
            source.write_text(text, encoding='utf-8')
            env = os.environ.copy()
            env.pop('RFORT_TIME_PASSES', None)
            if enabled:
                env['RFORT_TIME_PASSES'] = '1'
            return subprocess.run([COMPILER, '-c', source, '-o', directory / 'example.obj'],
                                  cwd=directory, env=env, capture_output=True,
                                  text=True, timeout=60)

    def test_quiet_by_default(self):
        result = self.compile('implicit none\nprint *, 42\nend\n', False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, '')
        self.assertEqual(result.stderr, '')

    def test_opt_in_pass_timings(self):
        result = self.compile('implicit none\nprint *, 42\nend\n', True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, '')
        for name in ('parse', 'prepare_program', 'array_calls::resolve', 'initialized', 'ir'):
            self.assertIn(f'[pass] start {name}', result.stderr)
            self.assertRegex(result.stderr, rf'\[pass\] end {name}: \d+\.\d+s')

    def test_failed_parse_still_reports_time(self):
        result = self.compile('program\n', True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('[pass] end parse:', result.stderr)
        self.assertIn('parse failed', result.stderr)


if __name__ == '__main__':
    unittest.main()

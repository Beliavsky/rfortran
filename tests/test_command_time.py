"""Elapsed timing must preserve program output and failure exit status."""
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER


class CommandTime(unittest.TestCase):
    def invoke(self, source, *options):
        with tempfile.TemporaryDirectory(prefix='rfort-time-') as temporary:
            directory = Path(temporary)
            path = directory / 'example.f90'
            path.write_text(source, encoding='utf-8')
            env = os.environ.copy()
            env.pop('RFORT_TIME_PASSES', None)
            result = subprocess.run([COMPILER, str(path), *options], cwd=directory,
                                    env=env, capture_output=True, text=True, timeout=60)
            self.object_exists = path.with_suffix('.obj' if os.name == 'nt' else '.o').exists()
            return result

    def times(self, result, execution=False):
        phases = ['Compilation', 'Overall']
        if execution:
            phases.insert(1, 'Execution')
        values = {}
        for phase in phases:
            matches = re.findall(rf'^{phase}:\s+(\d+\.\d{{3}}) s(?: \(failed\))?$',
                                 result.stderr, re.MULTILINE)
            self.assertEqual(len(matches), 1, result.stderr)
            values[phase] = float(matches[0])
        if not execution:
            self.assertNotIn('Execution:', result.stderr)
        self.assertGreaterEqual(values['Overall'] + .002,
                                sum(v for k, v in values.items() if k != 'Overall'))

    def test_compile_only(self):
        result = self.invoke('implicit none\nprint *, 42\nend\n', '--time', '-c')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.object_exists)
        self.assertEqual(result.stdout, '')
        self.times(result)

    def test_build_and_run(self):
        result = self.invoke('implicit none\nprint *, 42\nend\n', '--run', '--time')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), '42')
        self.times(result, execution=True)
        self.assertNotIn('(failed)', result.stderr)

    def test_compile_failure_skips_execution(self):
        result = self.invoke('program\n', '--time', '--run')
        self.assertNotEqual(result.returncode, 0)
        self.times(result)
        self.assertRegex(result.stderr, r'Compilation: .*\(failed\)')

    def test_execution_failure_preserves_status(self):
        result = self.invoke('implicit none\nstop 7\nend\n', '--time', '--run')
        self.assertEqual(result.returncode, 7, result.stderr)
        self.times(result, execution=True)
        self.assertRegex(result.stderr, r'Execution: .*\(failed\)')
        self.assertNotRegex(result.stderr, r'Compilation: .*\(failed\)')

    def test_no_timings_by_default(self):
        result = self.invoke('implicit none\nprint *, 42\nend\n', '--run')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), '42')
        self.assertEqual(result.stderr, '')

    def test_multiple_sources_and_link_failure(self):
        with tempfile.TemporaryDirectory(prefix='rfort-time-modules-') as temporary:
            directory = Path(temporary)
            module = directory / 'constants.f90'
            main = directory / 'main.f90'
            module.write_text('module constants\ninteger, parameter :: n=42\nend module\n',
                              encoding='utf-8')
            main.write_text('use constants\nprint *, n\nend\n', encoding='utf-8')
            result = subprocess.run([COMPILER, '--time', module, main, '--run'],
                                    cwd=directory, capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), '42')
            self.times(result, execution=True)

            invalid_object = directory / 'invalid.obj'
            invalid_object.write_text('not an object file', encoding='utf-8')
            result = subprocess.run([COMPILER, 'link', invalid_object, '--time'],
                                    cwd=directory, capture_output=True, text=True, timeout=60)
            self.assertNotEqual(result.returncode, 0)
            self.times(result)
            self.assertRegex(result.stderr, r'Compilation: .*\(failed\)')


if __name__ == '__main__':
    unittest.main()

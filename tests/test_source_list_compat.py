"""Source-list harness checks; no Fortran compiler is invoked."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from run_source_list_compat import copy_sources, invoke, source_list


class SourceListTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='rfort source list fixtures ')
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.root = self.directory / 'sources'
        self.root.mkdir()

    def test_order_quotes_comments_and_duplicate_entries(self):
        for name in ('module file.f90', 'main.f90'):
            (self.root / name).write_text('end\n', encoding='utf-8')
        listing = self.root / 'example_files.txt'
        listing.write_text('\ufeff# ordered inputs\n"module file.f90"\n! comment\nmain.f90\nmain.f90\n', encoding='utf-8')
        sources = source_list(listing, self.root)
        self.assertEqual([p.name for p in sources], ['module file.f90', 'main.f90', 'main.f90'])

    def test_missing_empty_and_external_sources_are_rejected(self):
        (self.directory / 'outside.f90').write_text('end\n', encoding='utf-8')
        listing = self.root / 'example_files.txt'
        for text in ('', 'missing.f90', '../outside.f90'):
            listing.write_text(text, encoding='utf-8')
            with self.assertRaises(ValueError):
                source_list(listing, self.root)

    def test_transitive_include_snapshot_and_no_source_tree_writes(self):
        source = self.root / 'main.f90'
        source.write_text("include 'nested/one.inc'\ninclude 'missing.inc'\nend\n", encoding='utf-8')
        (self.root / 'nested').mkdir()
        (self.root / 'nested/one.inc').write_text("include 'two.inc'\n", encoding='utf-8')
        (self.root / 'nested/two.inc').write_text('integer::n\n', encoding='utf-8')
        destination = self.directory / 'build inputs'
        before = source.read_bytes()
        arguments, copied, missing = copy_sources([source, source], self.root, destination)
        self.assertEqual(arguments, [str(destination / 'main.f90')]*2)
        self.assertEqual(len(copied), 3)
        self.assertEqual(missing, [dict(source='main.f90', include='missing.inc')])
        self.assertEqual((destination / 'nested/two.inc').read_text(), 'integer::n\n')
        self.assertEqual(source.read_bytes(), before)

    def test_timeout_retains_output_and_failure_retains_exit_status(self):
        original = subprocess.run
        def timed_out(*args, **kwargs):
            raise subprocess.TimeoutExpired(['compiler'], 1, output=b'partial output', stderr=b'partial diagnostic')
        try:
            subprocess.run = timed_out
            result = invoke(['compiler'], self.directory, 1)
        finally:
            subprocess.run = original
        self.assertTrue(result['timed_out'])
        self.assertEqual(result['stderr'], 'partial diagnostic')
        result = invoke([sys.executable, '-S', '-c', 'raise SystemExit(3)'], self.directory, 10)
        self.assertEqual(result['returncode'], 3)
        self.assertFalse(result['timed_out'])


if __name__ == '__main__':
    unittest.main()

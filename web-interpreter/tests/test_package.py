"""Check deployable assets without building Rust or requiring browser tools."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('site_package', Path(__file__).resolve().parents[1] / 'package.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class PackageTests(unittest.TestCase):
    def test_editor_urls_change_when_their_contents_change(self):
        with tempfile.TemporaryDirectory() as work:
            source, output = Path(work) / 'source', Path(work) / 'output'
            source.mkdir()
            for name in MODULE.FILES:
                (source / name).write_text(name, encoding='utf-8')
            (source / 'index.html').write_text('<script src="indent.js"></script><script src="app.js"></script><link href="style.css"><a href="https://example.com/app.js">external</a>', encoding='utf-8')
            (source / 'runtime').mkdir()
            (source / 'runtime/rfort-browser.wasm').write_bytes(b'test artifact')
            (source / 'vendor').mkdir()
            (source / 'vendor/highlight.js').write_text('highlight', encoding='utf-8')
            MODULE.package(output, source)
            before = (output / 'index.html').read_text(encoding='utf-8')
            for name in ('indent.js', 'app.js', 'style.css'):
                digest = hashlib.sha256((source / name).read_bytes()).hexdigest()[:16]
                self.assertIn(f'{name}?v={digest}', before)
            self.assertIn('href="https://example.com/app.js"', before)
            self.assertTrue((output / '.nojekyll').is_file())
            self.assertTrue((output / 'vendor/highlight.js').is_file())
            (source / 'indent.js').write_text('fixed Enter handling', encoding='utf-8')
            MODULE.package(output, source)
            after = (output / 'index.html').read_text(encoding='utf-8')
            self.assertNotEqual(before, after)
            self.assertEqual((output / 'indent.js').read_text(encoding='utf-8'), 'fixed Enter handling')

    def test_missing_runtime_is_an_actionable_error(self):
        with tempfile.TemporaryDirectory() as work:
            with self.assertRaisesRegex(FileNotFoundError, 'build.py'):
                MODULE.package(Path(work) / 'output', Path(work) / 'source')


if __name__ == '__main__':
    unittest.main()

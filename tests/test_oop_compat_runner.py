"""Offline tests for the external tutorial harness (no compiler/network needed)."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import run_oop_compat as runner


def record(code=0, **kwargs):
    return dict(returncode=code, stdout='', stderr='', **kwargs)


def success(output='1 T 2.0'):
    return {'compile': record(), 'run': dict(returncode=0, stdout=output, stderr='')}


class OopRunnerTests(unittest.TestCase):
    def test_pinned_offline_cache_and_tampering(self):
        with tempfile.TemporaryDirectory() as tmp:
            cache = Path(tmp)
            data = b'module original\nend module\n'
            digest = hashlib.sha256(data).hexdigest()
            spec = dict(sha256=digest, url='https://example.invalid/file')
            with self.assertRaisesRegex(ValueError, 'not cached'):
                runner.cached_resource(spec, cache)
            (cache / digest).write_bytes(data)
            self.assertEqual(runner.cached_resource(spec, cache), data)
            (cache / digest).write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError, 'mismatch'):
                runner.cached_resource(spec, cache, download=True)

    def test_download_verifies_before_caching(self):
        from io import BytesIO
        with tempfile.TemporaryDirectory() as tmp:
            cache = Path(tmp)
            spec = dict(sha256='a'*64, url='https://example.invalid/file')
            with patch.object(runner.urllib.request, 'urlopen', return_value=BytesIO(b'bad')):
                with self.assertRaisesRegex(ValueError, 'mismatch'):
                    runner.cached_resource(spec, cache, download=True)
            self.assertEqual(list(cache.iterdir()), [])

    def test_extract_only_selected_module(self):
        data = b'## 5. Test\n```fortran\nprogram ignored\nend\n```\n```\nmodule kept\nend module\n```\n## 6. Next\n```\nmodule wrong\nend\n```'
        self.assertEqual(runner.extract_module(data, 5), b'module kept\nend module\n')
        with self.assertRaisesRegex(ValueError, 'not found'):
            runner.extract_module(data, 7)

    def test_prepare_order_and_path_validation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'first.f90').write_text('module first\nend module')
            (root / 'main.f90').write_text('program main\nend')
            case = {'files': [{'driver': 'first.f90'}, {'driver': 'main.f90'}]}
            files = runner.prepare(case, {}, root, root / 'cache', False)
            self.assertEqual([name for name, _ in files], ['first.f90', 'main.f90'])
            case['files'].append({'driver': '../escape.f90'})
            with self.assertRaisesRegex(ValueError, 'outside'):
                runner.prepare(case, {}, root, root / 'cache', False)

    def test_known_failure_requires_phase_and_signature(self):
        case = dict(known_failure=dict(phase='compile', pattern='PRIVATE unsupported'))
        actual = {'compile': dict(returncode=1, stderr='PRIVATE unsupported', stdout='')}
        self.assertEqual(runner.classify(case, success(), actual), 'known_unsupported')
        actual['compile']['stderr'] = 'unrelated crash'
        self.assertEqual(runner.classify(case, success(), actual), 'rfort_compile_failed')
        actual = {'compile': record(), 'run': dict(returncode=1, stderr='PRIVATE unsupported', stdout='')}
        self.assertEqual(runner.classify(case, success(), actual), 'rfort_run_failed')

    def test_timeouts_and_launch_errors_are_not_known_gaps(self):
        case = dict(known_failure=dict(phase='compile', pattern='unsupported'))
        for flag, suffix in [('timed_out', 'timeout'), ('launch_error', 'launch_error')]:
            actual = {'compile': dict(returncode=None, stderr='unsupported', stdout='', **{flag: True})}
            self.assertEqual(runner.classify(case, success(), actual), 'rfort_compile_' + suffix)

    def test_reference_failure_mismatch_and_new_support(self):
        case = dict(known_failure=dict(phase='compile', pattern='unsupported'))
        self.assertEqual(runner.classify(case, {'compile': record(1)}, success()), 'reference_failed')
        self.assertEqual(runner.classify(case, success(), success('2 T 2.0')), 'output_mismatch')
        self.assertEqual(runner.classify(case, success(), success('1 .TRUE. 2.0000001')), 'newly_supported')
        self.assertEqual(runner.classify({}, success(), success()), 'matched_reference')

    def test_fresh_compiler_directories_and_build_order(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            calls = []
            def invoke(command, directory, timeout):
                calls.append((command, directory))
                if len(command) > 1:
                    (directory / 'module.rmod').write_text('cached')
                return record()
            files = [('m.f90', b'module m\nend module'), ('p.f90', b'program p\nend')]
            with patch.object(runner, 'invoke', side_effect=invoke):
                runner.build_run('rfort', [], files, root / 'rfort', 1, 1)
                runner.build_run('gfortran', [], files, root / 'gfortran', 1, 1)
            self.assertEqual(calls[0][0][1:3], ['m.f90', 'p.f90'])
            self.assertNotEqual(calls[0][1], calls[2][1])
            with self.assertRaises(FileExistsError):
                runner.build_run('rfort', [], files, root / 'rfort', 1, 1)

    def test_manifest_contract(self):
        path = runner.ROOT / 'tests/oop_compat/manifest.json'
        manifest = json.loads(path.read_text())
        ids = [c['id'] for c in manifest['cases']]
        self.assertEqual(len(ids), len(set(ids)))
        for spec in manifest['resources'].values():
            self.assertRegex(spec['sha256'], r'^[0-9a-f]{64}$')
            self.assertTrue(all(spec.get(k) for k in ['url', 'attribution', 'license', 'license_url']))
        for case in manifest['cases']:
            for item in case['files']:
                if 'driver' in item:
                    self.assertTrue((path.parent / item['driver']).is_file())
                else:
                    self.assertIn(item['resource'], manifest['resources'])
            for notice in case['notices']:
                self.assertIn(notice, manifest['resources'])


if __name__ == '__main__':
    unittest.main()

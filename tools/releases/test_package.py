"""Release helpers: binary architecture, checksums and license provenance."""
import hashlib
import contextlib
import io
import json
from pathlib import Path
import struct
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import package_windows as package


class PackageTests(unittest.TestCase):
    def test_pe64_imports_and_wrong_architecture(self):
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary) / 'rfort.exe'
            data = bytearray(2048)
            data[:2] = b'MZ'
            struct.pack_into('<I', data, 0x3c, 0x80)
            data[0x80:0x84] = b'PE\0\0'
            struct.pack_into('<HH', data, 0x84, 0x8664, 1)
            struct.pack_into('<H', data, 0x94, 240)
            struct.pack_into('<H', data, 0x98, 0x20b)
            struct.pack_into('<I', data, 0x98+120, 0x1000)
            struct.pack_into('<IIII', data, 0x98+240+8, 1024, 0x1000, 1024, 512)
            struct.pack_into('<I', data, 512+12, 0x1100)
            data[768:781] = b'KERNEL32.dll\0'
            binary.write_bytes(data)
            self.assertEqual(package.pe_imports(binary), ['KERNEL32.dll'])
            struct.pack_into('<H', data, 0x84, 0x14c)
            binary.write_bytes(data)
            with self.assertRaisesRegex(ValueError, 'x86-64'):
                package.pe_imports(binary)

    def test_checksum_covers_actual_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            file = Path(temporary) / 'archive.zip'
            file.write_bytes(b'first build')
            before = package.sha256(file)
            file.write_bytes(b'second build')
            self.assertNotEqual(before, package.sha256(file))
            self.assertEqual(package.sha256(file), hashlib.sha256(b'second build').hexdigest())

    def test_workspace_license_origin_and_repository_boundaries(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            packages = []
            for name in ('parent', 'member'):
                directory = root / name
                directory.mkdir()
                (directory / 'Cargo.toml').touch()
                packages.append({'name': name, 'version': '1', 'id': name,
                                 'manifest_path': str(directory / 'Cargo.toml'),
                                 'repository': 'https://example.com/project', 'license': 'MIT'})
            (root / 'parent/LICENSE-MIT').write_text('Repository license text', encoding='utf-8')
            metadata = {'packages': packages, 'resolve': {'nodes': [{'id': x} for x in ('parent', 'member')]}}
            with patch.object(package, 'capture', return_value=json.dumps(metadata)):
                package.dependency_licenses(root / 'output')
            index = json.loads((root / 'output/index.json').read_text(encoding='utf-8'))
            self.assertEqual(next(p for p in index if p['name'] == 'member')['license_text_origin'], 'parent-1')
            self.assertEqual((root / 'output/member-1/LICENSE-MIT').read_text(encoding='utf-8'), 'Repository license text')
            packages[1]['repository'] = 'https://example.com/unrelated'
            with patch.object(package, 'capture', return_value=json.dumps(metadata)):
                with self.assertRaisesRegex(RuntimeError, 'Missing license texts'):
                    package.dependency_licenses(root / 'other-output')

    def test_official_package_rejects_untracked_changes(self):
        with patch.object(package, 'capture', side_effect=['abc123', '?? src/uncommitted.rs']):
            with self.assertRaisesRegex(RuntimeError, 'clean checkout'):
                package.package(Path('unused.exe'), Path('unused-output'))

    def test_missing_repository_can_use_identical_vcs_commit_and_spdx(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            packages = []
            for name in ('parent', 'member'):
                directory = root / name
                directory.mkdir()
                (directory / 'Cargo.toml').touch()
                (directory / '.cargo_vcs_info.json').write_text(json.dumps({'git': {'sha1': 'a'*40}}), encoding='utf-8')
                packages.append({'name': name, 'version': '1', 'id': name,
                                 'manifest_path': str(directory / 'Cargo.toml'), 'license': 'MIT',
                                 'repository': 'https://example.com/project' if name == 'parent' else None})
            (root / 'parent/LICENSE-MIT').write_text('Repository license text', encoding='utf-8')
            metadata = {'packages': packages, 'resolve': {'nodes': [{'id': x} for x in ('parent', 'member')]}}
            with patch.object(package, 'capture', return_value=json.dumps(metadata)):
                package.dependency_licenses(root / 'output')
            index = json.loads((root / 'output/index.json').read_text(encoding='utf-8'))
            member = next(p for p in index if p['name'] == 'member')
            self.assertEqual(member['license_text_origin'], 'parent-1')
            self.assertIn('VCS commit', member['license_match'])
            packages[1]['license'] = 'BSD-3-Clause'
            with patch.object(package, 'capture', return_value=json.dumps(metadata)):
                with self.assertRaisesRegex(RuntimeError, 'Missing license texts'):
                    package.dependency_licenses(root / 'mismatched-license')

    def test_clean_archive_contains_matching_source_and_checksums(self):
        # Test source/ZIP/checksum handling with an isolated fixture Git repo.
        # Execution itself is tested separately against the actual packaged exe.
        with tempfile.TemporaryDirectory() as temporary:
            root, output = Path(temporary) / 'project', Path(temporary) / 'artifacts'
            root.mkdir()
            (root / 'Cargo.toml').write_text('[package]\nname="rfortran"\nversion="1.2.3"\n', encoding='utf-8')
            (root / 'Cargo.lock').write_text('fixture dependency lock', encoding='utf-8')
            for name in ('LICENSE', 'LICENSE-MIT', 'LICENSING.md', 'README.md'):
                (root / name).write_text(name, encoding='utf-8')
            (root / 'docs').mkdir()
            for name in ('feature-status.md', 'language-support.md', 'usage.md', 'testing.md'):
                (root / 'docs' / name).write_text(name, encoding='utf-8')
            release_files = root / 'release-files'
            (release_files / 'examples').mkdir(parents=True)
            (release_files / 'INSTALL.md').write_text('Install @VERSION@', encoding='utf-8')
            (release_files / 'examples/hello.f90').write_text('print *,1\nend\n', encoding='utf-8')
            (release_files / 'examples/hello.obj').write_bytes(b'unwanted build artifact')
            (root / 'src/bin/tools').mkdir(parents=True)
            (root / 'src/bin/tools/objdump.exe').write_bytes(b'optional tool fixture')
            (root / '.gitattributes').write_text('src/bin/tools/objdump.exe export-ignore\n', encoding='utf-8')
            for command in (['git', 'init', '-q'], ['git', 'add', '.'],
                            ['git', '-c', 'user.name=Release test', '-c', 'user.email=release-test@example.invalid',
                             'commit', '-qm', 'fixture source']):
                subprocess.run(command, cwd=root, check=True, capture_output=True)
            binary = Path(temporary) / 'fixture.exe'
            binary.write_bytes(b'fixture executable')
            def licenses(directory):
                directory.mkdir()
                (directory / 'index.json').write_text('[]', encoding='utf-8')
            with patch.object(package, 'ROOT', root), patch.object(package, 'HERE', release_files), \
                    patch.object(package, 'pe_imports', return_value=['KERNEL32.dll']), \
                    patch.object(package, 'dependency_licenses', side_effect=licenses), \
                    patch.object(package, 'smoke', return_value={'checks': ['fixture check']}):
                with contextlib.redirect_stdout(io.StringIO()):
                    archive = package.package(binary, output)
            with zipfile.ZipFile(archive) as zip:
                info = json.loads(zip.read('rfortran-1.2.3-windows-x86_64/BUILD.json'))
                self.assertFalse(info['dirty'])
                self.assertEqual(info['binary_sha256'], package.sha256(binary))
                self.assertIn('rfortran-1.2.3-windows-x86_64/docs/feature-status.md', zip.namelist())
                self.assertFalse(any(name.endswith('.obj') for name in zip.namelist()))
            with tarfile.open(output / 'rfortran-1.2.3-source.tar.gz') as source:
                self.assertEqual(source.extractfile('rfortran-1.2.3/Cargo.lock').read(), b'fixture dependency lock')
                self.assertNotIn('rfortran-1.2.3/src/bin/tools/objdump.exe', source.getnames())
            for line in (output / 'SHA256SUMS.txt').read_text(encoding='utf-8').splitlines():
                digest, name = line.split('  ')
                self.assertEqual(digest, package.sha256(output / name))


if __name__ == '__main__':
    unittest.main()

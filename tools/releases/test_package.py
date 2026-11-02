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
import package_linux as linux


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
            (release_files / 'INSTALL-linux.md').write_text('Linux @VERSION@', encoding='utf-8')
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
            linux_output = Path(temporary) / 'linux-assets'
            elf = dict(needed=['libc.so.6'], minimum_glibc='2.34',
                       interpreter='/lib64/ld-linux-x86-64.so.2')
            def linux_licenses(directory, target):
                self.assertEqual(target, linux.TARGET)
                licenses(directory)
            with patch.object(package, 'ROOT', root), patch.object(linux, 'ROOT', root), \
                    patch.object(linux, 'HERE', release_files), \
                    patch.object(linux, 'dependency_licenses', side_effect=linux_licenses), \
                    patch.object(linux, 'elf_dependencies', return_value=elf), \
                    patch.object(linux, 'smoke', return_value={'checks': ['fixture check']}):
                with contextlib.redirect_stdout(io.StringIO()):
                    linux_archive = linux.package(binary, linux_output)
                    report = linux.verify(linux_archive, linux_output / 'smoke.json', info['commit'])
                self.assertEqual(report['commit'], info['commit'])
                with self.assertRaisesRegex(ValueError, 'revision'):
                    linux.verify(linux_archive, linux_output / 'bad.json', 'wrong revision')
            with tarfile.open(linux_archive) as tar:
                prefix = 'rfortran-1.2.3-linux-x86_64/'
                self.assertEqual(tar.getmember(prefix+'rfort').mode, 0o755)
                self.assertIn(prefix+'examples/hello.f90', tar.getnames())
                self.assertFalse(any(name.endswith('.obj') for name in tar.getnames()))
                self.assertEqual(tar.extractfile(prefix+'INSTALL.md').read(), b'Linux 1.2.3')
                build = json.load(tar.extractfile(prefix+'BUILD.json'))
                self.assertEqual(build['target'], linux.TARGET)
                self.assertEqual(build['elf'], elf)
            digest, name = (linux_output / 'SHA256SUMS-linux.txt').read_text().strip().split('  ')
            self.assertEqual(digest, package.sha256(linux_output / name))

    def test_linux_dependency_audit_rejects_wrong_arch_new_glibc_and_private_libraries(self):
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary) / 'rfort'
            header = bytearray(20)
            header[:6] = b'\x7fELF\x02\x01'
            struct.pack_into('<H', header, 18, 62)
            binary.write_bytes(header)
            def audit(dynamic, versions, interpreter='[Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]'):
                outputs = [subprocess.CompletedProcess([], 0, stdout=text)
                           for text in (dynamic, versions, interpreter)]
                with patch.object(linux.subprocess, 'run', side_effect=outputs):
                    return linux.elf_dependencies(binary)
            dynamic = '(NEEDED) Shared library: [libc.so.6]'
            self.assertEqual(audit(dynamic, 'GLIBC_2.2.5 GLIBC_2.34')['minimum_glibc'], '2.34')
            with self.assertRaisesRegex(ValueError, 'newer than baseline'):
                audit(dynamic, 'GLIBC_2.36')
            with self.assertRaisesRegex(ValueError, 'shared library'):
                audit(dynamic+'\n(NEEDED) Shared library: [libcustom.so]', 'GLIBC_2.34')
            with self.assertRaisesRegex(ValueError, 'RPATH'):
                audit(dynamic+'\n(RUNPATH) Library runpath: [/build/lib]', 'GLIBC_2.34')
            with self.assertRaisesRegex(ValueError, 'ELF interpreter'):
                audit(dynamic, 'GLIBC_2.34', '[Requesting program interpreter: /build/ld.so]')
            struct.pack_into('<H', header, 18, 183)
            binary.write_bytes(header)
            with self.assertRaisesRegex(ValueError, 'x86-64'):
                linux.elf_dependencies(binary)

    def test_linux_archive_rejects_traversal_and_links_before_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary) / 'bad.tar.gz'
            for name, kind in [('../escape', tarfile.REGTYPE), ('root/link', tarfile.SYMTYPE),
                               ('/absolute', tarfile.REGTYPE)]:
                with tarfile.open(archive, 'w:gz') as tar:
                    member = tarfile.TarInfo(name)
                    member.type = kind
                    member.linkname = '/outside'
                    tar.addfile(member)
                with patch.object(linux, 'smoke') as smoke:
                    with self.assertRaisesRegex(ValueError, 'Unsafe archive'):
                        linux.verify(archive, Path(temporary) / 'report.json')
                    smoke.assert_not_called()

    def test_linux_verification_rejects_tampered_binary_and_missing_execute_permission(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / 'candidate.tar.gz'
            info = dict(target=linux.TARGET, dirty=False, commit='test revision',
                        binary_sha256=hashlib.sha256(b'original').hexdigest())
            for content, mode, error in [(b'tampered', 0o755, 'checksum'),
                                         (b'original', 0o644, 'permission')]:
                with tarfile.open(archive, 'w:gz') as tar:
                    for name, data, permissions in [('root/BUILD.json', json.dumps(info).encode(), 0o644),
                                                     ('root/rfort', content, mode)]:
                        member = tarfile.TarInfo(name)
                        member.size, member.mode = len(data), permissions
                        tar.addfile(member, io.BytesIO(data))
                with patch.object(linux, 'smoke') as smoke:
                    with self.assertRaisesRegex(ValueError, error):
                        linux.verify(archive, root / 'report.json')
                    smoke.assert_not_called()


if __name__ == '__main__':
    unittest.main()

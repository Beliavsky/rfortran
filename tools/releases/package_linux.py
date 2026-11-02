"""Package Linux x86-64 releases and verify the downloaded archive on a fresh host."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tarfile
import tempfile
import tomllib

from package_windows import capture, dependency_licenses, sha256
from smoke import smoke

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
TARGET = 'x86_64-unknown-linux-gnu'
BASELINE = '2.35'
SYSTEM_LIBRARIES = {'libc.so.6', 'libm.so.6', 'libgcc_s.so.1', 'libpthread.so.0',
                    'libdl.so.2', 'librt.so.1', 'libutil.so.1', 'libresolv.so.2'}


def elf_dependencies(binary):
    with Path(binary).open('rb') as file:
        header = file.read(20)
    if (len(header) != 20 or header[:6] != b'\x7fELF\x02\x01'
            or struct.unpack_from('<H', header, 18)[0] != 62):
        raise ValueError('Expected a little-endian ELF64 x86-64 executable.')

    def readelf(option):
        return subprocess.run(['readelf', option, str(binary)], check=True,
                              capture_output=True, text=True,
                              env=dict(os.environ, LC_ALL='C')).stdout

    dynamic = readelf('-dW')
    needed = sorted(set(re.findall(r'\(NEEDED\).*?\[(.*?)\]', dynamic)))
    if not needed or set(needed) - SYSTEM_LIBRARIES:
        raise ValueError(f'Unexpected shared library dependencies: {needed}')
    if re.search(r'\((?:RPATH|RUNPATH)\)', dynamic):
        raise ValueError('Release executable must not contain RPATH/RUNPATH.')
    versions = set(re.findall(r'\bGLIBC_([\d.]+)\b', readelf('-VW')))
    if not versions:
        raise ValueError('No glibc symbol requirements found.')
    minimum = max(versions, key=lambda s: tuple(map(int, s.split('.'))))
    if tuple(map(int, minimum.split('.'))) > tuple(map(int, BASELINE.split('.'))):
        raise ValueError(f'Binary requires glibc {minimum}, newer than baseline {BASELINE}.')
    interpreter = re.search(r'Requesting program interpreter:\s*([^\]]+)', readelf('-lW'))
    if not interpreter or interpreter[1] != '/lib64/ld-linux-x86-64.so.2':
        raise ValueError('Expected the standard Linux x86-64 glibc ELF interpreter.')
    return dict(needed=needed, minimum_glibc=minimum, interpreter=interpreter[1])


def package(binary, output, allow_dirty=False):
    binary, output = Path(binary).resolve(), Path(output).resolve()
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    revision = capture(['git', 'rev-parse', 'HEAD'])
    dirty = bool(capture(['git', 'status', '--porcelain', '--untracked-files=normal']))
    if dirty and not allow_dirty:
        raise RuntimeError('Use a clean checkout; --allow-dirty is for local PREVIEW checks only.')
    dependencies = elf_dependencies(binary)
    output.mkdir(parents=True, exist_ok=True)
    name = f'rfortran-{version}-linux-x86_64' + ('-PREVIEW' if dirty else '')
    archive = output / f'{name}.tar.gz'
    with tempfile.TemporaryDirectory(prefix='rfort linux package ') as temporary:
        stage = Path(temporary) / name
        stage.mkdir()
        shutil.copyfile(binary, stage / 'rfort')
        for file in ('LICENSE', 'LICENSE-MIT', 'LICENSING.md', 'README.md'):
            shutil.copyfile(ROOT / file, stage / file)
        (stage / 'docs').mkdir()
        for file in ('feature-status.md', 'language-support.md', 'usage.md', 'testing.md'):
            shutil.copyfile(ROOT / 'docs' / file, stage / 'docs' / file)
        (stage / 'examples').mkdir()
        for file in sorted((HERE / 'examples').glob('*.f90')):
            shutil.copyfile(file, stage / 'examples' / file.name)
        install = (HERE / 'INSTALL-linux.md').read_text(encoding='utf-8')
        (stage / 'INSTALL.md').write_text(install.replace('@VERSION@', version), encoding='utf-8')
        dependency_licenses(stage / 'THIRD-PARTY-LICENSES', target=TARGET)
        info = dict(version=version, target=TARGET, commit=revision, dirty=dirty,
                    source=f'https://github.com/Beliavsky/rfortran/tree/{revision}',
                    binary_sha256=sha256(binary), elf=dependencies,
                    compatibility_baseline=f'Linux x86-64, glibc {BASELINE} or newer',
                    rustc=capture(['rustc', '-vV']),
                    disassembler='external objdump; not bundled')
        (stage / 'BUILD.json').write_text(json.dumps(info, indent=2)+'\n', encoding='utf-8')

        def permissions(member):
            member.mode = 0o755 if member.isdir() or member.name == f'{name}/rfort' else 0o644
            member.uid = member.gid = 0
            member.uname = member.gname = ''
            return member

        with tarfile.open(archive, 'w:gz') as tar:
            tar.add(stage, arcname=name, filter=permissions)
    (output / 'SHA256SUMS-linux.txt').write_text(f'{sha256(archive)}  {archive.name}\n', encoding='utf-8')
    print(f'Packaged {archive}; archive execution tests are required before release.')
    return archive


def verify(archive, report, expected_commit=None):
    archive = Path(archive).resolve()
    with tempfile.TemporaryDirectory(prefix='rfort extracted linux distribution ') as temporary:
        unpacked = Path(temporary)
        # Release archives contain only ordinary files/directories under one root.
        # Reject links and traversal before extracting any bytes or executing code.
        with tarfile.open(archive, 'r:gz') as tar:
            members = tar.getmembers()
            roots = set()
            names = set()
            for member in members:
                parts = member.name.split('/')
                if (not parts[0] or any(p in ('', '.', '..') for p in parts)
                        or '\\' in member.name or ':' in member.name
                        or member.name in names or not (member.isfile() or member.isdir())):
                    raise ValueError(f'Unsafe archive member: {member.name}')
                roots.add(parts[0])
                names.add(member.name)
            if len(roots) != 1:
                raise ValueError('Expected one distribution directory.')
            for member in members:
                path = unpacked / member.name
                if member.isdir():
                    path.mkdir(parents=True, exist_ok=True)
                else:
                    path.parent.mkdir(parents=True, exist_ok=True)
                    with tar.extractfile(member) as source, path.open('wb') as destination:
                        shutil.copyfileobj(source, destination)
                    path.chmod(member.mode & 0o777)
        stage = unpacked / roots.pop()
        info = json.loads((stage / 'BUILD.json').read_text(encoding='utf-8'))
        if info['target'] != TARGET or info['dirty']:
            raise ValueError('Expected a clean Linux x86-64 release build.')
        if expected_commit and info['commit'] != expected_commit:
            raise ValueError('Archive source revision does not match the workflow revision.')
        if sha256(stage / 'rfort') != info['binary_sha256']:
            raise ValueError('Extracted executable checksum does not match BUILD.json.')
        if not any(m.name == f'{stage.name}/rfort' and m.mode & 0o111 for m in members):
            raise ValueError('Executable permission missing from archive.')
        if elf_dependencies(stage / 'rfort') != info['elf']:
            raise ValueError('Extracted ELF dependencies do not match BUILD.json.')
        results = smoke(stage / 'rfort', stage / 'examples', info['version'])
        results.update(archive=archive.name, archive_sha256=sha256(archive),
                       target=TARGET, commit=info['commit'], elf=info['elf'])
        report = Path(report)
        report.parent.mkdir(parents=True, exist_ok=True)
        report.write_text(json.dumps(results, indent=2)+'\n', encoding='utf-8')
        print(f'Extracted Linux archive: {len(results["checks"])} checks passed.')
        return results


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    build = commands.add_parser('build')
    build.add_argument('--binary', type=Path, required=True)
    build.add_argument('--output', type=Path, default=ROOT / 'target/release-assets')
    build.add_argument('--allow-dirty', action='store_true')
    check = commands.add_parser('verify')
    check.add_argument('--archive', type=Path, required=True)
    check.add_argument('--report', type=Path, required=True)
    check.add_argument('--expected-commit')
    args = parser.parse_args()
    if args.command == 'build':
        package(args.binary, args.output, args.allow_dirty)
    else:
        verify(args.archive, args.report, args.expected_commit)

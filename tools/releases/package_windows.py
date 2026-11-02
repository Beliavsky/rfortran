"""Build and verify a Windows x64 release archive from a prebuilt rfort.exe."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import tomllib
import zipfile

from smoke import smoke

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
TARGET = 'x86_64-pc-windows-msvc'
SYSTEM_DLLS = {'kernel32.dll', 'ntdll.dll', 'advapi32.dll', 'bcrypt.dll', 'bcryptprimitives.dll',
               'userenv.dll', 'ws2_32.dll', 'user32.dll', 'crypt32.dll',
               'secur32.dll', 'shell32.dll', 'ole32.dll', 'oleaut32.dll',
               'gdi32.dll', 'psapi.dll', 'setupapi.dll', 'version.dll',
               'winmm.dll', 'dbghelp.dll', 'msvcrt.dll', 'cfgmgr32.dll',
               'api-ms-win-core-synch-l1-2-0.dll'}


def capture(command):
    return subprocess.run(command, cwd=ROOT, capture_output=True, encoding='utf-8',
                          errors='replace', check=True).stdout.strip()


def sha256(path):
    with Path(path).open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def pe_imports(path):
    """Read the PE64 import table, without requiring objdump on the user's PATH."""
    data = Path(path).read_bytes()
    if data[:2] != b'MZ':
        raise ValueError('Expected a Windows executable.')
    pe = struct.unpack_from('<I', data, 0x3c)[0]
    if data[pe:pe+4] != b'PE\0\0' or struct.unpack_from('<H', data, pe+4)[0] != 0x8664:
        raise ValueError('Expected an x86-64 Windows executable.')
    optional = pe+24
    if struct.unpack_from('<H', data, optional)[0] != 0x20b:
        raise ValueError('Expected PE32+ format.')
    count = struct.unpack_from('<H', data, pe+6)[0]
    section_start = optional + struct.unpack_from('<H', data, pe+20)[0]

    def offset(rva):
        for i in range(count):
            size, address, raw_size, raw = struct.unpack_from('<IIII', data, section_start+40*i+8)
            if address <= rva < address+max(size, raw_size):
                return raw+rva-address
        raise ValueError(f'Invalid import address {rva:x}')

    imports = []
    rva = struct.unpack_from('<I', data, optional+120)[0]
    if not rva:
        return imports
    descriptor = offset(rva)
    while any(data[descriptor:descriptor+20]):
        name_rva = struct.unpack_from('<I', data, descriptor+12)[0]
        name_start = offset(name_rva)
        imports.append(data[name_start:data.index(b'\0', name_start)].decode('ascii'))
        descriptor += 20
    return sorted(set(imports))


def dependency_licenses(destination):
    metadata = json.loads(capture(['cargo', 'metadata', '--locked', '--format-version', '1',
                                   '--filter-platform', TARGET]))
    ids = {node['id'] for node in metadata['resolve']['nodes']}
    packages = [p for p in metadata['packages'] if p['id'] in ids and p['name'] != 'rfortran']
    found, peers, commits, vcs_peers = {}, {}, {}, {}
    for package in packages:
        root = Path(package['manifest_path']).parent
        files = [p for p in root.iterdir() if p.is_file() and
                 p.name.lower().startswith(('license', 'copying', 'notice', 'unlicense', 'copyright'))]
        if package.get('license_file'):
            license_file = root / package['license_file']
            if license_file.is_file():
                files.append(license_file)
        files = sorted(set(files))
        found[package['id']] = files
        vcs = root / '.cargo_vcs_info.json'
        commit = json.loads(vcs.read_text(encoding='utf-8')).get('git', {}).get('sha1') if vcs.is_file() else None
        commits[package['id']] = commit
        if files and package.get('repository'):
            peers[(package['repository'], package.get('license'))] = (package, files)
        if files and commit:
            vcs_peers[(commit, package.get('license'))] = (package, files)
    index = []
    for package in sorted(packages, key=lambda p: (p['name'], p['version'])):
        origin, files = package, found[package['id']]
        match = 'own license texts'
        if not files:
            # Some workspace members omit the repository-wide license texts.
            # Only use another crate's texts when both repository and SPDX match.
            origin, files = peers.get((package.get('repository'), package.get('license')), (None, []))
            match = 'same repository and SPDX expression'
        if not files and commits[package['id']]:
            origin, files = vcs_peers.get((commits[package['id']], package.get('license')), (None, []))
            match = 'identical Cargo VCS commit and SPDX expression'
        if not files:
            raise RuntimeError(f'Missing license texts for {package["name"]} {package["version"]}')
        directory = destination / f'{package["name"]}-{package["version"]}'
        directory.mkdir(parents=True)
        for file in files:
            shutil.copyfile(file, directory / file.name)
        index.append({key: package.get(key) for key in ('name', 'version', 'license', 'repository', 'authors')} |
                     {'license_text_origin': f'{origin["name"]}-{origin["version"]}',
                      'license_match': match, 'source_commit': commits[package['id']]})
    (destination / 'index.json').write_text(json.dumps(index, indent=2)+'\n', encoding='utf-8')


def package(binary, output, allow_dirty=False):
    binary, output = Path(binary).resolve(), Path(output).resolve()
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    revision = capture(['git', 'rev-parse', 'HEAD'])
    dirty = bool(capture(['git', 'status', '--porcelain', '--untracked-files=normal']))
    if dirty and not allow_dirty:
        raise RuntimeError('Commit release changes and use a clean checkout; --allow-dirty is for local PREVIEW checks only.')
    imports = pe_imports(binary)
    unexpected = [name for name in imports if name.lower() not in SYSTEM_DLLS]
    if unexpected:
        raise RuntimeError(f'Non-system DLL dependencies: {unexpected}; build with +crt-static.')
    output.mkdir(parents=True, exist_ok=True)
    suffix = '-PREVIEW' if dirty else ''
    name = f'rfortran-{version}-windows-x86_64{suffix}'
    archive = output / f'{name}.zip'
    with tempfile.TemporaryDirectory(prefix='rfort package ') as temporary:
        stage = Path(temporary) / name
        stage.mkdir()
        shutil.copyfile(binary, stage / 'rfort.exe')
        for file in ('LICENSE', 'LICENSE-MIT', 'LICENSING.md', 'README.md'):
            shutil.copyfile(ROOT / file, stage / file)
        (stage / 'docs').mkdir()
        for file in ('feature-status.md', 'language-support.md', 'usage.md', 'testing.md'):
            shutil.copyfile(ROOT / 'docs' / file, stage / 'docs' / file)
        (stage / 'examples').mkdir()
        for file in sorted((HERE / 'examples').glob('*.f90')):
            shutil.copyfile(file, stage / 'examples' / file.name)
        install = (HERE / 'INSTALL.md').read_text(encoding='utf-8')
        (stage / 'INSTALL.md').write_text(install.replace('@VERSION@', version), encoding='utf-8')
        dependency_licenses(stage / 'THIRD-PARTY-LICENSES')
        info = {'version': version, 'target': TARGET, 'commit': revision, 'dirty': dirty,
                'source': f'https://github.com/Beliavsky/rfortran/tree/{revision}',
                'binary_sha256': sha256(binary), 'dll_imports': imports,
                'disassembler': 'external objdump/dumpbin; not bundled'}
        (stage / 'BUILD.json').write_text(json.dumps(info, indent=2)+'\n', encoding='utf-8')
        candidate = Path(temporary) / f'{name}.zip'
        with zipfile.ZipFile(candidate, 'w', zipfile.ZIP_DEFLATED) as zip:
            for file in sorted(stage.rglob('*')):
                if file.is_file():
                    zip.write(file, file.relative_to(stage.parent))
        # Test the actual extracted archive, never the compiler in target/release.
        unpacked = Path(temporary) / 'unpacked distribution'
        with zipfile.ZipFile(candidate) as zip:
            zip.extractall(unpacked)
        extracted = unpacked / name
        report = smoke(extracted / 'rfort.exe', extracted / 'examples', version)
        shutil.copyfile(candidate, archive)
        smoke_report = output / f'{name}-smoke.json'
        smoke_report.write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    assets = [archive, smoke_report]
    if not dirty:
        source = output / f'rfortran-{version}-source.tar.gz'
        subprocess.run(['git', 'archive', '--format=tar.gz', f'--prefix=rfortran-{version}/',
                        '-o', str(source), revision], cwd=ROOT, check=True)
        assets.append(source)
    checksums = output / 'SHA256SUMS.txt'
    checksums.write_text(''.join(f'{sha256(file)}  {file.name}\n' for file in assets), encoding='utf-8')
    print(json.dumps({'archive': str(archive), 'checksums': str(checksums),
                      'checks_passed': len(report['checks']), 'preview': dirty}, indent=2))
    return archive


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, default=ROOT / 'target/release-assets')
    parser.add_argument('--allow-dirty', action='store_true', help='Local preview, never an official release')
    args = parser.parse_args()
    package(args.binary, args.output, args.allow_dirty)

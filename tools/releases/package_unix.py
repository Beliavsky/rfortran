"""Shared Unix release staging, archive validation, and smoke-test reporting."""
import json
from pathlib import Path
import shutil
import tarfile
import tempfile
import tomllib

from package_windows import sha256


def package(binary, output, allow_dirty=False, *, target, platform_name, checksum_name,
            install_name, compatibility, inspect, metadata_key, root, here, licenses, capture):
    binary, output = Path(binary).resolve(), Path(output).resolve()
    version = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    revision = capture(['git', 'rev-parse', 'HEAD'])
    dirty = bool(capture(['git', 'status', '--porcelain', '--untracked-files=normal']))
    if dirty and not allow_dirty:
        raise RuntimeError('Use a clean checkout; --allow-dirty is for local PREVIEW checks only.')
    dependencies = inspect(binary)
    output.mkdir(parents=True, exist_ok=True)
    name = f'rfortran-{version}-{platform_name}' + ('-PREVIEW' if dirty else '')
    archive = output / f'{name}.tar.gz'
    with tempfile.TemporaryDirectory(prefix='rfort unix package ') as temporary:
        stage = Path(temporary) / name
        stage.mkdir()
        shutil.copyfile(binary, stage / 'rfort')
        for file in ('LICENSE', 'LICENSE-MIT', 'LICENSING.md', 'README.md'):
            shutil.copyfile(root / file, stage / file)
        (stage / 'docs').mkdir()
        for file in ('feature-status.md', 'language-support.md', 'usage.md', 'testing.md'):
            shutil.copyfile(root / 'docs' / file, stage / 'docs' / file)
        (stage / 'examples').mkdir()
        for file in sorted((here / 'examples').glob('*.f90')):
            shutil.copyfile(file, stage / 'examples' / file.name)
        install = (here / install_name).read_text(encoding='utf-8')
        (stage / 'INSTALL.md').write_text(install.replace('@VERSION@', version), encoding='utf-8')
        licenses(stage / 'THIRD-PARTY-LICENSES', target=target)
        info = dict(version=version, target=target, commit=revision, dirty=dirty,
                    source=f'https://github.com/Beliavsky/rfortran/tree/{revision}',
                    binary_sha256=sha256(binary), **{metadata_key: dependencies},
                    compatibility_baseline=compatibility,
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
    (output / checksum_name).write_text(f'{sha256(archive)}  {archive.name}\n', encoding='utf-8')
    print(f'Packaged {archive}; archive execution tests are required before release.')
    return archive


def verify(archive, report, expected_commit=None, *, target, inspect, metadata_key, smoke):
    archive = Path(archive).resolve()
    with tempfile.TemporaryDirectory(prefix='rfort extracted unix distribution ') as temporary:
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
        if info['target'] != target or info['dirty']:
            raise ValueError(f'Expected a clean {target} release build.')
        if expected_commit and info['commit'] != expected_commit:
            raise ValueError('Archive source revision does not match the workflow revision.')
        if sha256(stage / 'rfort') != info['binary_sha256']:
            raise ValueError('Extracted executable checksum does not match BUILD.json.')
        if not any(m.name == f'{stage.name}/rfort' and m.mode & 0o111 for m in members):
            raise ValueError('Executable permission missing from archive.')
        if inspect(stage / 'rfort') != info[metadata_key]:
            raise ValueError('Extracted native dependencies do not match BUILD.json.')
        results = smoke(stage / 'rfort', stage / 'examples', info['version'])
        results.update(archive=archive.name, archive_sha256=sha256(archive),
                       target=target, commit=info['commit'], **{metadata_key: info[metadata_key]})
        report = Path(report)
        report.parent.mkdir(parents=True, exist_ok=True)
        report.write_text(json.dumps(results, indent=2)+'\n', encoding='utf-8')
        print(f'Extracted {target} archive: {len(results["checks"])} checks passed.')
        return results



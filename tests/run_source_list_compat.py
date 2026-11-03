"""Compile ordered *files.txt groups with a snapshot of rfort and gfortran.

Never rebuilds a compiler or executes a compiled program. All generated files
are confined to a new output directory, with independent compiler workspaces.
"""
import argparse
from collections import Counter
from datetime import datetime, timezone
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
INCLUDE = re.compile(r'^\s*(?:#\s*)?include\s*[\'"]([^\'"]+)[\'"]', re.I | re.M)


def sha256(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def source_list(path, root):
    sources = []
    for line in path.read_text(encoding='utf-8-sig').splitlines():
        name = line.strip()
        if not name or name.startswith(('#', '!')):
            continue
        name = name.strip('\"\'').replace('\\', os.sep)
        source = (path.parent / name).resolve()
        if not source.is_relative_to(root) or not source.is_file():
            raise ValueError(f'Missing source or path outside --root: {name}')
        sources.append(source)
    if not sources:
        raise ValueError('Empty source list')
    return sources


def copy_sources(sources, root, destination):
    copied, pending, missing = {}, list(sources), []
    while pending:
        source = pending.pop(0)
        if source in copied:
            continue
        relative = source.relative_to(root)
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
        copied[source] = dict(path=str(relative), sha256=sha256(target))
        for name in INCLUDE.findall(source.read_text(encoding='utf-8-sig', errors='replace')):
            candidates = [(source.parent / name).resolve(), (root / name).resolve()]
            match = next((p for p in candidates if p.is_relative_to(root) and p.is_file()), None)
            if match:
                pending.append(match)
            else:
                missing.append(dict(source=str(relative), include=name))
    return [str(destination / p.relative_to(root)) for p in sources], list(copied.values()), missing


def invoke(command, cwd, timeout):
    start = time.perf_counter()
    try:
        process = subprocess.run(list(map(str, command)), cwd=cwd, input='',
                                 capture_output=True, text=True, errors='replace', timeout=timeout)
        return dict(command=list(map(str, command)), seconds=time.perf_counter()-start,
                    returncode=process.returncode, timed_out=False,
                    stdout=process.stdout, stderr=process.stderr)
    except subprocess.TimeoutExpired as error:
        decode = lambda value: value.decode(errors='replace') if isinstance(value, bytes) else value or ''
        return dict(command=list(map(str, command)), seconds=time.perf_counter()-start,
                    returncode=None, timed_out=True, stdout=decode(error.stdout), stderr=decode(error.stderr))


def first_error(result):
    lines = (result.get('stderr', '')+'\n'+result.get('stdout', '')).splitlines()
    return next((line.strip() for line in lines if re.match(r'^\s*(?:fatal )?error\b', line, re.I)), '')


def save(report, directory):
    report['counts'] = dict(Counter(c['status'] for c in report['cases']))
    (directory / 'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    lines = ['# Ordered-source compilation sample', '',
             f"Sources: `{report['source_root']}`", '',
             'Compilation and linking only; no generated program was executed.',
             'Source-list order is preserved. rfort uses its default optimization; '
             'gfortran uses -O0 -ffree-line-length-none. Each compiler has an isolated workspace.',
             'The rfort executable was snapshotted; the source tree and compiler code were not modified.', '',
             f"rfort SHA256: `{report['rfort_sha256']}`", '',
             '| Source list | Files | Result | rfort seconds | gfortran seconds | First rfort diagnostic |',
             '|---|---:|---|---:|---:|---|']
    for case in report['cases']:
        r, g = case.get('rfort', {}), case.get('gfortran', {})
        clean = lambda value: str(value).replace('|', '/').replace('\n', ' ')
        lines.append(f"| {case['list']} | {case.get('source_count', 0)} | {case['status']} | "
                     f"{r.get('seconds', 0):.3f} | {g.get('seconds', 0):.3f} | "
                     f"{clean(first_error(r) or case.get('error', ''))} |")
    lines += ['', 'Counts: '+str(report['counts']), '',
              'Full diagnostics and commands are in report.json and each case directory. '
              'A compilation failure is not automatically a compiler bug; inspect the reference result '
              'and check the source and required flags. Compilation success does not establish runtime correctness.']
    (directory / 'report.md').write_text('\n'.join(lines)+'\n', encoding='utf-8')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--compiler', type=Path, default=ROOT / 'target/release' / ('rfort.exe' if os.name == 'nt' else 'rfort'))
    parser.add_argument('--lists', nargs='+', default=['*files.txt'], help='Source-list names or globs')
    parser.add_argument('--limit', type=int, default=20)
    parser.add_argument('--compile-timeout', type=float, default=90)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args(argv)
    root, compiler = args.root.resolve(), args.compiler.resolve()
    if not root.is_dir() or not compiler.is_file() or args.limit < 1 or args.compile_timeout <= 0:
        parser.error('root/compiler must exist and limits must be positive')
    selected = [p for p in sorted(root.glob('*files.txt'))
                if any(fnmatch.fnmatchcase(p.name, pattern) for pattern in args.lists)][:args.limit]
    if not selected:
        parser.error('no matching source lists')
    directory = (args.output or ROOT / 'target/source-list-compat' /
                 (datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')+'-'+uuid.uuid4().hex[:6])).resolve()
    if directory.exists() and (not directory.is_dir() or any(directory.iterdir())):
        parser.error('--output must be a new or empty directory')
    directory.mkdir(parents=True, exist_ok=True)
    snapshot = directory / compiler.name
    shutil.copyfile(compiler, snapshot)
    snapshot.chmod(snapshot.stat().st_mode | 0o111)
    reference = shutil.which('gfortran')
    report = dict(source_root=str(root), rfort_original=str(compiler), rfort_snapshot=str(snapshot),
                  rfort_sha256=sha256(snapshot), created_utc=datetime.now(timezone.utc).isoformat(),
                  compile_timeout=args.compile_timeout, executed_programs=False,
                  rfort_version=invoke([snapshot, '--version'], directory, 15),
                  gfortran_version=invoke([reference, '--version'], directory, 15) if reference else None,
                  cases=[])
    print(f'Snapshot: {snapshot}\nReport: {directory / "report.md"}', flush=True)
    for path in selected:
        case = dict(list=path.name, list_sha256=sha256(path))
        work = directory / path.stem
        try:
            sources = source_list(path, root)
            copied, hashes, missing = copy_sources(sources, root, work / 'sources')
            case.update(source_count=len(sources), source_order=[str(p.relative_to(root)) for p in sources],
                        copied_files=hashes, missing_includes=missing)
            for name, tool in [('rfort', snapshot), ('gfortran', reference)]:
                if tool is None:
                    continue
                build = work / name
                build.mkdir()
                command = [str(tool)] + (['-O0', '-ffree-line-length-none'] if name == 'gfortran' else [])
                command += copied + ['-o', str(build / ('program.exe' if os.name == 'nt' else 'program'))]
                result = invoke(command, build, args.compile_timeout)
                case[name] = result
                for stream in ('stdout', 'stderr'):
                    (build / (stream+'.txt')).write_text(result[stream], encoding='utf-8')
            r, g = case['rfort'], case.get('gfortran')
            if r['timed_out']:
                case['status'] = 'rfort_timeout'
            elif r['returncode'] == 0:
                case['status'] = 'both_compile' if g and g['returncode'] == 0 else 'rfort_compiles'
            else:
                case['status'] = 'rfort_failure' if g and g['returncode'] == 0 else 'both_failed' if g else 'rfort_failed_no_reference'
        except (OSError, ValueError) as error:
            case.update(status='setup_failed', error=str(error))
        report['cases'].append(case)
        save(report, directory)
        print(f'{path.name:22} {case["status"]:28} {first_error(case.get("rfort", {}))}', flush=True)
    print('Counts:', report['counts'], flush=True)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())

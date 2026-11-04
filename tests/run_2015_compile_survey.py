"""Survey separate module/main compilation using ordered Fortran source manifests.

Compile only: no linking or program execution. Identical dependency prefixes are
cached within this run. Sources and rfort are snapshotted; originals are untouched.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time

DEFAULTS = 'cxb acf ar1 bcx elm pse sim qpt abc dsa stt stv tta bvn mom reg'.split()


def timings(text):
    active, completed = [], []
    for line in text.splitlines():
        if line.startswith('[pass] start '):
            active.append(line[13:])
        elif line.startswith('[pass] end '):
            match = re.match(r'\[pass\] end (.*): ([0-9.]+)s$', line)
            if match:
                completed.append({'name': match[1], 'seconds': float(match[2])})
                if active and active[-1] == match[1]:
                    active.pop()
    return active, completed


def invoke(command, cwd, log, timeout, environment):
    start = time.perf_counter()
    code = None
    with log.open('w', encoding='utf-8') as stream:
        try:
            result = subprocess.run(command, cwd=cwd, stdin=subprocess.DEVNULL,
                                    stdout=stream, stderr=stream, timeout=timeout,
                                    env=environment)
            code = result.returncode
            status = 'passed' if code == 0 else 'failed'
        except subprocess.TimeoutExpired:
            status = 'timeout'
    elapsed = time.perf_counter() - start
    text = log.read_text(encoding='utf-8', errors='replace')
    active, passes = timings(text)
    diagnostics = [line.strip() for line in text.splitlines()
                   if re.match(r'\s*(?:fatal error|error|Error|Semantic|Parse):', line)]
    return dict(status=status, seconds=elapsed, returncode=code, command=command,
                cwd=str(cwd), log=str(log), active_passes=active, passes=passes,
                first_error=diagnostics[0] if diagnostics else '')


def copy_interfaces(source, destination):
    if source:
        for suffix in ('*.mod', '*.smod', '*.rmod'):
            for file in source.glob(suffix):
                shutil.copy2(file, destination / file.name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path('C:/fortran/2015'))
    parser.add_argument('--compiler', type=Path, default=Path('target/release/rfort.exe'))
    parser.add_argument('--reference', default='gfortran')
    parser.add_argument('--timeout', type=float, default=120)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('manifests', nargs='*', help='Manifest names or short stems')
    args = parser.parse_args()
    root, output = args.root.resolve(), args.output.resolve()
    reference = shutil.which(args.reference)
    if not reference:
        parser.error('reference compiler not found')
    output.mkdir(parents=True, exist_ok=False)
    source_dir = output / 'sources'
    source_dir.mkdir()
    compiler = output / args.compiler.name
    shutil.copy2(args.compiler, compiler)
    names = args.manifests or DEFAULTS
    manifests, snapshots = [], {}

    def snapshot(relative):
        original = (root / relative).resolve()
        relative = original.relative_to(root)
        key = relative.as_posix()
        if key in snapshots:
            return
        data = original.read_bytes()
        target = source_dir / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        snapshots[key] = hashlib.sha256(data).hexdigest()
        for include in re.findall(r'^\s*(?:include|#\s*include)\s*[\"\']([^\"\']+)[\"\']',
                                  data.decode('utf-8', errors='replace'), re.I | re.M):
            local = relative.parent / include
            snapshot(local if (root / local).is_file() else Path(include))

    for name in names:
        name = name if name.endswith('.txt') else name + '_files.txt'
        try:
            text = (root / name).read_text(encoding='utf-8-sig')
            files = [word.strip('\"\'') for line in text.splitlines()
                     for word in line.split('!')[0].split()]
            if not files:
                raise ValueError('empty manifest')
            for file in files:
                snapshot(Path(file))
            main_text = (source_dir / files[-1]).read_text(errors='replace')
            if not re.search(r'^\s*program\s+\w+', main_text, re.I | re.M):
                raise ValueError('last source has no explicit PROGRAM; inspect manifest')
            shutil.copy2(root / name, source_dir / name)
            manifests.append(dict(name=name, files=files))
        except (OSError, ValueError) as error:
            manifests.append(dict(name=name, input_error=str(error)))

    environment = dict(os.environ, RFORT_TIME_PASSES='1')
    environment.pop('RUST_LOG', None)
    environment.pop('RFORT_DUMP_LOWERED', None)
    version = subprocess.run([reference, '--version'], capture_output=True, text=True).stdout.splitlines()[0]
    report = dict(root=str(root), compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(),
                  reference=reference, reference_version=version, timeout=args.timeout,
                  options={'rfort': ['-O', '2'], 'gfortran': ['-O2', '-ffree-line-length-none']},
                  sources=snapshots, manifests=manifests, invocations=[], cases=[])

    def save():
        (output / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')

    cache = {}
    for manifest in manifests:
        name = manifest['name']
        if 'input_error' in manifest:
            report['cases'].append(dict(manifest=name, status='input_error', error=manifest['input_error']))
            save()
            continue
        files = manifest['files']
        for tool, executable, flags in [('rfort', str(compiler), ['-O', '2']),
                                        ('gfortran', reference, ['-O2', '-ffree-line-length-none'])]:
            previous = None
            prefix, dependencies = [], []
            case = dict(manifest=name, compiler=tool, main=files[-1], dependencies=dependencies)
            for index, file in enumerate(files):
                is_main = index == len(files) - 1
                prefix.append((file, snapshots[Path(file).as_posix()]))
                key = (tool, hashlib.sha256(json.dumps(prefix).encode()).hexdigest())
                cached = not is_main and key in cache
                if cached:
                    work, record = cache[key]
                else:
                    work = output / ('cases' if is_main else 'modules') / tool / (Path(name).stem if is_main else key[1][:20])
                    work.mkdir(parents=True)
                    copy_interfaces(previous, work)
                    command = [executable, *flags, '-c', str(source_dir / file), '-I', str(work),
                               '-I', str(source_dir), '-J', str(work), '-o', str(work / 'unit.o')]
                    record = invoke(command, work, work / 'compile.log', args.timeout, environment)
                    record.update(compiler=tool, source=file, stage='main' if is_main else 'dependency')
                    report['invocations'].append(record)
                    if not is_main:
                        cache[key] = (work, record)
                    save()
                if is_main:
                    case.update(status=record['status'], main_result=record)
                else:
                    dependencies.append(dict(source=file, cached=cached, result=record))
                    if record['status'] != 'passed':
                        case.update(status='dependency_' + record['status'], blocked_by=file)
                        break
                    previous = work
            report['cases'].append(case)
            save()
            result = case.get('main_result', dependencies[-1]['result'] if dependencies else {})
            print(f'{name:18} {tool:8} {case["status"]:20} {result.get("seconds", 0):8.3f}s '
                  + '/'.join(result.get('active_passes', [])), flush=True)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())

"""Optional, checksum-pinned external OOP tutorial comparison with gfortran."""
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
import tempfile
import time
import urllib.request

from run_ofort_compat import equivalent

ROOT = Path(__file__).resolve().parents[1]


def cached_resource(spec, cache, download=False):
    """Read verified bytes; never silently replace a corrupt cache entry."""
    digest = spec['sha256']
    if not re.fullmatch('[0-9a-f]{64}', digest):
        raise ValueError('invalid SHA256 in manifest')
    path = cache / digest
    if path.exists():
        data = path.read_bytes()
    elif download:
        with urllib.request.urlopen(spec['url'], timeout=45) as response:
            data = response.read()
    else:
        raise ValueError('source not cached; rerun with --download: ' + spec['url'])
    if hashlib.sha256(data).hexdigest() != digest:
        raise ValueError('SHA256 mismatch: ' + spec['url'])
    cache.mkdir(parents=True, exist_ok=True)
    if not path.exists():
        path.write_bytes(data)
    return data


def extract_module(data, section):
    text = data.decode('utf-8-sig').replace('\r\n', '\n')
    match = re.search(r'^## ' + str(section) + r'\..*?(?=^## |\Z)', text, re.M | re.S)
    if not match:
        raise ValueError(f'Markdown section {section} not found')
    for block in re.findall(r'```[^\n]*\n(.*?)```', match[0], re.S):
        if re.search(r'^module\s+', block, re.M | re.I):
            return block.encode('utf-8')
    raise ValueError(f'no module code block in section {section}')


def prepare(case, manifest, manifest_dir, cache, download):
    files = []
    for item in case['files']:
        if 'driver' in item:
            path = (manifest_dir / item['driver']).resolve()
            if not path.is_relative_to(manifest_dir.resolve()):
                raise ValueError('driver outside manifest directory')
            name, data = path.name, path.read_bytes()
        else:
            name = item['name']
            data = cached_resource(manifest['resources'][item['resource']], cache, download)
            if 'section' in item:
                data = extract_module(data, item['section'])
        if Path(name).name != name or name in {n for n, _ in files}:
            raise ValueError('unsafe or duplicate build filename: ' + name)
        files.append((name, data))
    return files


def invoke(command, cwd, timeout):
    started = time.perf_counter()
    record = dict(command=list(map(str, command)), returncode=None, stdout='', stderr='')
    try:
        result = subprocess.run(record['command'], cwd=cwd, input='', capture_output=True,
                                text=True, errors='replace', timeout=timeout)
        record.update(returncode=result.returncode, stdout=result.stdout, stderr=result.stderr)
    except subprocess.TimeoutExpired as error:
        decode = lambda s: s.decode(errors='replace') if isinstance(s, bytes) else s or ''
        record.update(timed_out=True, stdout=decode(error.stdout), stderr=decode(error.stderr))
    except OSError as error:
        record['launch_error'] = str(error)
    record['seconds'] = time.perf_counter() - started
    return record


def build_run(compiler, flags, files, directory, compile_timeout, run_timeout):
    directory.mkdir(parents=True)
    for name, data in files:
        (directory / name).write_bytes(data)
    exe = directory / ('example.exe' if os.name == 'nt' else 'example')
    result = {'compile': invoke([compiler, *flags, *(n for n, _ in files), '-o', exe],
                                directory, compile_timeout)}
    if result['compile']['returncode'] == 0:
        result['run'] = invoke([exe], directory, run_timeout)
    return result


def failure(result):
    for phase in ['compile', 'run']:
        record = result.get(phase)
        if record is None:
            return phase, 'missing_result'
        if record.get('launch_error'):
            return phase, 'launch_error'
        if record.get('timed_out'):
            return phase, 'timeout'
        if record['returncode'] != 0:
            return phase, 'failed'
    return None


def classify(case, reference, actual):
    if failure(reference):
        return 'reference_failed'
    failed = failure(actual)
    known = case.get('known_failure')
    if failed:
        phase, reason = failed
        diagnostic = actual.get(phase, {}).get('stderr', '') + actual.get(phase, {}).get('stdout', '')
        if (known and reason == 'failed' and phase == known['phase']
                and re.search(known['pattern'], diagnostic, re.I)):
            return 'known_unsupported'
        return 'rfort_' + phase + '_' + reason
    if not equivalent(actual['run']['stdout'], reference['run']['stdout']):
        return 'output_mismatch'
    return 'newly_supported' if known else 'matched_reference'


def write_report(report, path):
    report['counts'] = dict(Counter(c['status'] for c in report['cases']))
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    lines = ['# External OOP tutorial comparison', '',
             'Generated: ' + report['started'], '',
             'Counts: ' + str(report['counts']), '',
             '| Case | Result | Features |', '|---|---|---|']
    for case in report['cases']:
        lines.append(f"| {case['id']} | {case['status']} | {case['features']} |")
    lines += ['', 'See the JSON report for commands, diagnostics, timings, source checksums, and compiler versions.',
              'Known failures require the recorded phase and diagnostic signature; timeouts and output mismatches are failures.']
    path.with_suffix('.md').write_text('\n'.join(lines) + '\n', encoding='utf-8')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, default=ROOT / 'tests/oop_compat/manifest.json')
    parser.add_argument('--compiler', default=str(ROOT / 'target/release' / ('rfort.exe' if os.name == 'nt' else 'rfort')))
    parser.add_argument('--reference', default='gfortran')
    parser.add_argument('--cache', type=Path, default=ROOT / 'target/oop-compat/cache')
    parser.add_argument('--report', type=Path, default=ROOT / 'target/oop-compat/report.json')
    parser.add_argument('--download', action='store_true', help='download missing pinned sources; otherwise offline')
    parser.add_argument('--case', action='append', default=[], help='case ID or glob; repeatable')
    parser.add_argument('--list', action='store_true', help='list selected cases without downloading or compiling')
    parser.add_argument('--compile-timeout', type=float, default=120)
    parser.add_argument('--run-timeout', type=float, default=15)
    args = parser.parse_args()
    manifest = json.loads(args.manifest.read_text(encoding='utf-8'))
    if manifest['schema_version'] != 1:
        parser.error('unsupported manifest version')
    cases = [c for c in manifest['cases'] if not args.case or any(fnmatch.fnmatchcase(c['id'], p) for p in args.case)]
    if not cases:
        parser.error('no cases selected')
    if args.compile_timeout <= 0 or args.run_timeout <= 0:
        parser.error('timeouts must be positive')
    if args.list:
        for case in cases:
            print(case['id'] + ': ' + case['features'])
        return 0
    compiler, reference = shutil.which(args.compiler), shutil.which(args.reference)
    if not compiler or not reference:
        parser.error('rfort and reference compiler must exist (use --compiler and --reference)')
    compiler, reference = str(Path(compiler).resolve()), str(Path(reference).resolve())
    report_path = args.report.resolve()
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report = dict(started=datetime.now(timezone.utc).isoformat(), manifest=str(args.manifest.resolve()),
                  resources=manifest['resources'], versions={}, cases=[])
    for name, exe in [('rfort', compiler), ('gfortran', reference)]:
        report['versions'][name] = invoke([exe, '--version'], ROOT, 15)
    # Retain builds for diagnosis; each sweep and compiler gets fresh module files.
    sweep = Path(tempfile.mkdtemp(prefix='build-', dir=report_path.parent))
    report['build_directory'] = str(sweep)
    for index, case in enumerate(cases):
        row = dict(id=case['id'], features=case['features'], expectation=case.get('known_failure'))
        try:
            files = prepare(case, manifest, args.manifest.resolve().parent, args.cache.resolve(), args.download)
            # Cache license notices alongside the external sources where provided.
            for key in case.get('notices', []):
                notice = cached_resource(manifest['resources'][key], args.cache.resolve(), args.download)
                notices = sweep / 'notices'
                notices.mkdir(exist_ok=True)
                (notices / key).write_bytes(notice)
            row['source_hashes'] = {n: hashlib.sha256(d).hexdigest() for n, d in files}
            folder = sweep / str(index)
            row['gfortran'] = build_run(reference, ['-O0', '-fcheck=all'], files, folder / 'gfortran', args.compile_timeout, args.run_timeout)
            if failure(row['gfortran']):
                row['status'] = 'reference_failed'
            else:
                row['rfort'] = build_run(compiler, ['-O0'], files, folder / 'rfort', args.compile_timeout, args.run_timeout)
                row['status'] = classify(case, row['gfortran'], row['rfort'])
        except (OSError, ValueError, KeyError) as error:
            row.update(status='source_error', error=str(error))
        report['cases'].append(row)
        write_report(report, report_path)
        print(row['id'], row['status'], flush=True)
    good = {'matched_reference', 'newly_supported', 'known_unsupported'}
    return int(any(c['status'] not in good for c in report['cases']))


if __name__ == '__main__':
    raise SystemExit(main())

"""Run rfort on runtime tests of the gfortran test suite.

Usage: python tools/gfortran_testsuite.py SUITE OUTDIR [--sample N] [--seed S]
                                          [--compiler PATH] [-O LEVEL]

SUITE is the testsuite directory of https://github.com/lazy-fortran/gfortran-testsuite.
All of gfortran.fortran-torture/execute is run, plus a random sample of N
gfortran.dg tests marked { dg-do run } that need no extra sources, options,
modules from other tests, OpenMP or coarrays. Each test is built with
rfort -O LEVEL build and run; a test passes when it builds and exits with
status 0 (the suite's tests call abort or STOP n on failure). A test that fails
at run time is also built with gfortran: if it fails there too it is reported
as GFORTRAN_FAIL rather than counted against rfort. dg-output patterns are not
checked.

OUTDIR receives results.csv (one row per test), summary.md (counts and the
most common compile errors) and logs/ (the output of every test not passing).
"""
import argparse
import csv
import os
from pathlib import Path
import random
import re
import shutil
import subprocess
import sys
import tempfile
import time
from collections import Counter

ROOT = Path(__file__).resolve().parent.parent
SKIP = re.compile(r'dg-additional-sources|dg-additional-files|dg-require|dg-shouldfail|dg-options|'
                  r'dg-error|dg-compile-aux-modules|-fcoarray|-fopenmp|-fopenacc|dg-skip-if|dg-xfail', re.I)
INCLUDE = re.compile(r'^\s*include\s*[\'"]', re.I | re.M)


def candidates(suite, sample, seed):
    torture = sorted(p for p in (suite / 'gfortran.fortran-torture/execute').iterdir()
                     if p.suffix.lower() in ('.f90', '.f', '.f95', '.f03', '.f08'))
    torture = [p for p in torture if not INCLUDE.search(read(p))]
    dg = []
    for p in sorted((suite / 'gfortran.dg').iterdir()):
        if p.suffix.lower() not in ('.f90', '.f', '.f95', '.f03', '.f08'):
            continue
        text = read(p)
        if 'dg-do run' in text and not SKIP.search(text) and not INCLUDE.search(text):
            dg.append(p)
    random.Random(seed).shuffle(dg)
    return torture + sorted(dg[:sample])


def read(path):
    return path.read_text(encoding='utf-8', errors='replace')


def run(command, cwd, timeout):
    start = time.time()
    try:
        r = subprocess.run(command, cwd=cwd, capture_output=True, text=True,
                           errors='replace', timeout=timeout)
        return r.returncode, r.stdout, r.stderr, time.time() - start
    except subprocess.TimeoutExpired as e:
        out = e.stdout.decode(errors='replace') if isinstance(e.stdout, bytes) else (e.stdout or '')
        return None, out, 'timeout', time.time() - start


def first_error(text):
    for line in text.splitlines():
        if re.search(r'error|not supported|not yet|unsupported|panicked', line, re.I):
            return line.strip()
    lines = [l.strip() for l in text.splitlines() if l.strip()]
    return lines[0] if lines else ''


def error_kind(message):
    """The message with names, numbers and locations removed, to group similar errors."""
    m = re.sub(r'^.*?(error|Error)(\[\w+\])?:\s*', '', message)
    m = re.sub(r'`[^`]*`', '`_`', m)
    m = re.sub(r"'[^']*'", "'_'", m)
    m = re.sub(r'\b\d+\b', 'N', m)
    m = re.sub(r'[A-Za-z]:\\\S+|\S+\.f(90|95|03|08)?(:\d+)*', 'FILE', m)
    return m.strip()[:160]


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('suite', type=Path)
    parser.add_argument('outdir', type=Path)
    parser.add_argument('--sample', type=int, default=300)
    parser.add_argument('--seed', type=int, default=1)
    parser.add_argument('--compiler', type=Path,
                        default=ROOT / 'target/release' / ('rfort.exe' if os.name == 'nt' else 'rfort'))
    parser.add_argument('-O', dest='opt', default='2')
    args = parser.parse_args()
    suite = args.suite.resolve()
    compiler = args.compiler.resolve()
    out = args.outdir.resolve()
    logs = out / 'logs'
    shutil.rmtree(logs, ignore_errors=True)
    logs.mkdir(parents=True)
    tests = candidates(suite, args.sample, args.seed)
    gfortran = shutil.which('gfortran')
    exe_name = 'test.exe' if os.name == 'nt' else 'test'
    rows = []
    start = time.time()
    for i, test in enumerate(tests, 1):
        rel = test.relative_to(suite).as_posix()
        sys.stdout.write(f'\r[{i}/{len(tests)}] {clock(time.time() - start)} {rel[:70]:<70}')
        sys.stdout.flush()
        with tempfile.TemporaryDirectory(prefix='gts-') as tmp:
            work = Path(tmp)
            source = work / test.name
            shutil.copyfile(test, source)
            code, so, se, build_time = run([compiler, '-O', args.opt, 'build', test.name, '-o', exe_name], work, 120)
            detail = ''
            run_time = 0.0
            if code != 0:
                status = 'BUILD_TIMEOUT' if code is None else ('COMPILER_CRASH' if 'panicked' in se else 'COMPILE_FAIL')
                detail = first_error(se + so)
                log = f'$ rfort build\n{so}{se}'
            else:
                code, so, se, run_time = run([work / exe_name], work, 20)
                if code == 0:
                    status, log = 'PASS', ''
                else:
                    status = 'RUN_TIMEOUT' if code is None else 'RUN_FAIL'
                    detail = f'exit {code}: ' + first_error(se + so)
                    log = f'$ ./test (exit {code})\n{so}{se}'
                    if gfortran:
                        g = run([gfortran, test.name, '-o', 'gref.exe'], work, 120)
                        r = run([work / 'gref.exe'], work, 20) if g[0] == 0 else g
                        if r[0] != 0:
                            status = 'GFORTRAN_FAIL'
                            log += f'\n$ gfortran (exit {r[0]})\n{r[1]}{r[2]}'
            if log:
                (logs / (rel.replace('/', '__') + '.txt')).write_text(log, encoding='utf-8')
        rows.append(dict(test=rel, status=status, build_s=f'{build_time:.1f}', run_s=f'{run_time:.1f}', detail=detail[:300]))
    print()
    with (out / 'results.csv').open('w', newline='', encoding='utf-8') as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    write_summary(out / 'summary.md', rows, args, compiler, time.time() - start)
    print((out / 'summary.md').read_text(encoding='utf-8').split('\n## ')[0])


def clock(seconds):
    seconds = int(seconds)
    return f'{seconds // 3600}:{seconds // 60 % 60:02}:{seconds % 60:02}'


def write_summary(path, rows, args, compiler, elapsed):
    lines = [f'# rfort on the gfortran test suite', '',
             f'Compiler: `{compiler}` at -O{args.opt}; {len(rows)} tests '
             f'(torture/execute plus a sample of {args.sample} gfortran.dg run tests, seed {args.seed}); '
             f'{clock(elapsed)} elapsed.', '']
    for group in ('gfortran.fortran-torture', 'gfortran.dg'):
        part = [r for r in rows if r['test'].startswith(group)]
        if not part:
            continue
        counts = Counter(r['status'] for r in part)
        scored = len(part) - counts['GFORTRAN_FAIL']
        lines.append(f'**{group}**: {counts["PASS"]} of {scored} pass '
                     f'({100 * counts["PASS"] // max(scored, 1)}%); '
                     + ', '.join(f'{k} {v}' for k, v in sorted(counts.items())))
        lines.append('')
    kinds = Counter(error_kind(r['detail']) for r in rows if r['status'] in ('COMPILE_FAIL', 'COMPILER_CRASH'))
    lines += ['## Most common compile errors', '', '| Tests | Error |', '|---:|---|']
    lines += [f'| {n} | {k.replace("|", "\\|")} |' for k, n in kinds.most_common(40)]
    lines += ['', '## Run-time failures', '', '| Test | Status | Detail |', '|---|---|---|']
    lines += [f'| {r["test"]} | {r["status"]} | {r["detail"][:150].replace("|", "\\|")} |'
              for r in rows if r['status'] in ('RUN_FAIL', 'RUN_TIMEOUT', 'BUILD_TIMEOUT', 'COMPILER_CRASH')]
    path.write_text('\n'.join(lines) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()

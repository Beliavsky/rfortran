#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Check and time serial/parallel rfort executables; never builds a compiler."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import fnmatch
import json
import math
import os
from pathlib import Path
import platform
import re
import shutil
import statistics
import sys
import uuid

import run as common

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
CASES = {
    'independent_math': 'Independent SQRT/SIN element updates',
    'scalar_reduce': '64-bit real scalar sum',
    'array_reduce': 'Eight-element 64-bit real array sum',
    'complex_reduce': 'Complex array sum and scalar product (64-bit components)',
    'matrix_loop': 'Two concurrent indices and column-major array writes',
    'section_results': 'PURE array results added to disjoint columns',
    'workspace_calls': 'PURE calls with automatic array workspaces',
}
PROFILES = {'tiny': (128, 1), 'smoke': (10000, 1), 'small': (100000, 2),
            'medium': (1000000, 4), 'large': (5000000, 8)}
HEAVY = {'array_reduce', 'complex_reduce', 'section_results', 'workspace_calls'}
DISPATCH_MIN = 4096
TRACE = re.compile(r'^rfort parallel: (\d+) threads\s*$', re.MULTILINE)


def profile(case, size):
    n, repetitions = PROFILES[size]
    if case in HEAVY and size not in ('tiny', 'smoke'):
        n = max(9000, n // 10)
    return dict(n=n, repetitions=repetitions)


def periodic_sum(n, offset, function):
    cycles, tail = divmod(n, 97)
    return cycles * math.fsum(function(r) for r in range(97)) + math.fsum(
        function((i + offset) % 97) for i in range(1, tail + 1))


def expected(case, n, repetitions, seed):
    steps = n + seed
    triangle = repetitions * (repetitions + 1) / 2
    weight = common.residue_sum(steps, seed, 97) / 97
    if case == 'independent_math':
        return triangle * periodic_sum(steps, seed, lambda r: math.sqrt(r + 1) + math.sin(r * .01))
    if case == 'scalar_reduce':
        return seed + repetitions * weight
    if case == 'array_reduce':
        return 8 * (seed + triangle * weight)
    if case == 'complex_reduce':
        p = (2 + 3j) * (1, 1j, -1, -1j)[steps * repetitions % 4]
        return 4 * (abs(seed + triangle * weight) + abs(2 - .5 * triangle * steps)) + p.real + 2 * p.imag
    if case in ('matrix_loop', 'section_results'):
        return triangle * sum(common.residue_sum(steps, seed + j, 97) for j in range(1, 5)) / 97
    if case == 'workspace_calls':
        squares = repetitions * (repetitions + 1) * (2 * repetitions + 1) / 6
        return squares * periodic_sum(steps, seed, lambda r: sum((r + j)**2 for j in range(1, 5)))
    raise ValueError(f'unknown benchmark: {case}')


def environment(threads, trace=False):
    env = os.environ.copy()
    # Do not inherit diagnostic settings or unrelated threaded-library defaults.
    for key in ('RFORT_PARALLEL_TRACE', 'RFORT_TIME_PASSES', 'RFORT_DUMP_LOWERED'):
        env.pop(key, None)
    for key in ('OMP_NUM_THREADS', 'OPENBLAS_NUM_THREADS', 'MKL_NUM_THREADS', 'VECLIB_MAXIMUM_THREADS'):
        env[key] = '1'
    env['RFORT_NUM_THREADS'] = str(threads)
    if trace:
        env['RFORT_PARALLEL_TRACE'] = '1'
    return env


def iterations(row, seed):
    return (row['profile']['n'] + seed) * (4 if row['benchmark'] == 'matrix_loop' else 1)


def validate_measurement(run, row, seed):
    if run['timeout']:
        return 'run_timeout'
    if run['returncode'] != 0:
        return 'run_failed'
    try:
        values = common.parse_output(run['stdout'])
        run['values'] = values
        reference = expected(row['benchmark'], **row['profile'], seed=seed)
        run['expected_checksum'] = reference
        if not math.isclose(values['BENCH_CHECKSUM'], reference, rel_tol=2e-8, abs_tol=1e-7) or values['BENCH_ERROR'] > 2e-8:
            return 'wrong_answer'
        if values['BENCH_SECONDS'] > run['seconds'] + max(.005, run['seconds'] * .01):
            raise ValueError('kernel time exceeds process wall time')
    except ValueError as error:
        run['validation_error'] = str(error)
        return 'invalid_output'
    return None


def probe_dispatch(row, args):
    probe = common.invoke([row['executable']], Path(row['directory']), args.timeout,
                          environment(row['threads'], trace=True), f'{args.seed}\n')
    row['probe'] = probe
    failure = validate_measurement(probe, row, args.seed)
    if failure:
        row['status'] = failure
        return
    observed = sorted(set(map(int, TRACE.findall(probe['stderr']))))
    required = (row['mode'] == 'parallel' and row['threads'] > 1
                and iterations(row, args.seed) >= DISPATCH_MIN)
    row['dispatch'] = dict(observed_threads=observed, required=required,
                           small_range=iterations(row, args.seed) < DISPATCH_MIN)
    if observed != ([row['threads']] if required else []):
        row['status'] = 'dispatch_failed'
        probe['validation_error'] = f'expected worker counts {[row["threads"]] if required else []}, got {observed}'


def build(case, size, mode, compiler, args, output):
    work = output / size / case / mode
    work.mkdir(parents=True)
    p = profile(case, size)
    source = HERE / 'parallel' / (case + '.f90')
    shutil.copy2(source, work / source.name)
    shutil.copy2(HERE / 'bench_support.f90', work / 'bench_support.f90')
    (work / 'bench_config.f90').write_text(
        'module bench_config\nimplicit none\ninteger,parameter :: n=' + str(p['n'])
        + ',repetitions=' + str(p['repetitions'])
        + '\nlogical,parameter :: warm_pool=' + ('.false.' if args.cold_pool else '.true.')
        + '\nend module\n', encoding='utf-8')
    executable = work / ('benchmark.exe' if os.name == 'nt' else 'benchmark')
    command = [compiler['path'], '-O' + args.optimization, '--parallel-report']
    if args.checks == 'none':
        command.append('--nocheck')
    if mode == 'parallel':
        command.append('--parallel')
    command += ['bench_config.f90', 'bench_support.f90', source.name, '-o', str(executable)]
    result = common.invoke(command, work, args.compile_timeout, environment(1))
    for stream in ('stdout', 'stderr'):
        (work / ('compile.' + stream + '.txt')).write_text(result[stream], encoding='utf-8')
    status = 'compiled'
    if result['timeout']:
        status = 'compile_timeout'
    elif result['returncode'] != 0 or not executable.is_file():
        status = 'compile_failed'
    elif mode == 'parallel' and ('native parallel worker emitted' not in result['stderr']
                                or 'serial fallback' in result['stderr']):
        status = 'serial_fallback'
    elif mode == 'serial' and 'native parallel worker emitted' in result['stderr']:
        status = 'unexpected_parallel_build'
    return dict(benchmark=case, size=size, mode=mode, profile=p, status=status,
                directory=str(work), executable=str(executable), build=result,
                source_hashes={n: common.digest(work / n) for n in ('bench_config.f90', 'bench_support.f90', source.name)})


def measure(row, args, warmup):
    result = common.invoke([row['executable']], Path(row['directory']), args.timeout,
                           environment(row['threads']), f'{args.seed}\n')
    row['warmups' if warmup else 'samples'].append(result)
    failure = validate_measurement(result, row, args.seed)
    if TRACE.search(result['stderr']):
        failure = 'unexpected_timed_trace'
    if failure:
        row['status'] = failure


def summarize(row, minimum):
    if row['status'] != 'compiled':
        return
    samples = row['samples']
    kernel = [r['values']['BENCH_SECONDS'] for r in samples]
    row['status'] = 'passed'
    row['timing'] = dict(median=statistics.median(kernel), minimum=min(kernel), maximum=max(kernel),
                         stdev=statistics.stdev(kernel) if len(kernel) > 1 else 0.0,
                         median_wall=statistics.median(r['seconds'] for r in samples),
                         reliable_duration=min(kernel) > 0 and statistics.median(kernel) >= minimum)


def speedups(rows):
    serial = next((r for r in rows if r['mode'] == 'serial' and r['status'] == 'passed'), None)
    for row in rows:
        if serial and serial['timing']['median'] > 0 and row['status'] == 'passed' and row['timing']['median'] > 0:
            row['speedup'] = serial['timing']['median'] / row['timing']['median']
            row['reliable_speedup'] = serial['timing']['reliable_duration'] and row['timing']['reliable_duration']


def cells(row):
    status = row['status']
    kernel = wall = ratio = '-'
    if status == 'passed':
        kernel = f"{row['timing']['median']:.6f}"
        wall = f"{row['timing']['median_wall']:.6f}"
        if not row['timing']['reliable_duration']:
            status += ' (short)'
        if row.get('dispatch', {}).get('small_range') and row['mode'] == 'parallel':
            status += ' (small range)'
        if 'speedup' in row:
            ratio = f"{row['speedup']:.2f}" + ('' if row['reliable_speedup'] else '*')
    return [row['benchmark'], row['size'], row['mode'], str(row['threads']), status, kernel, wall, ratio]


def markdown(report):
    lines = ['# rfort parallel benchmark report', '', f"UTC: {report['created_utc']}",
             f"Compiler SHA-256: `{report['compiler'].get('sha256', '-')}`", '',
             f"Optimization: `-O{report['settings']['optimization']}`; checks: `{report['settings']['checks']}`; pool: {'cold' if report['settings']['cold_pool'] else 'warm'}.", '',
             'Speedup = serial kernel median / configuration kernel median. * marks short timings.',
             'Dispatch probes are separate from timed samples. Parallel builds with one thread also differ in generated code.', '',
             '| Benchmark | Size | Mode | Threads | Status | Kernel seconds | Process seconds | Speedup |',
             '|---|---|---|---:|---|---:|---:|---:|']
    lines += ['| ' + ' | '.join(cells(r)) + ' |' for r in report['results']]
    lines += ['', 'Compilation commands/times, checks, worker probes, sources, and raw samples are in `report.json`.']
    return '\n'.join(lines) + '\n'


def positive(value):
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError('must be finite and positive')
    return number


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rfort', default=str(ROOT / 'target/release' / ('rfort.exe' if os.name == 'nt' else 'rfort')))
    parser.add_argument('--size', nargs='+', choices=PROFILES, default=['medium'])
    parser.add_argument('--threads', type=int, nargs='+', default=[1, 2, 4])
    parser.add_argument('--bench', nargs='+', default=['*'], help='benchmark names or quoted globs')
    parser.add_argument('--runs', type=int, default=5)
    parser.add_argument('--warmups', type=int, default=1)
    parser.add_argument('--seed', type=int, default=3)
    parser.add_argument('--checks', choices=['default', 'none'], default='default')
    parser.add_argument('--cold-pool', action='store_true', help='include first worker-pool creation in the timed region')
    parser.add_argument('-O', '--optimization', choices=['0', '1', '2', '3'], default='2')
    parser.add_argument('--timeout', type=positive, default=120)
    parser.add_argument('--compile-timeout', type=positive, default=300)
    parser.add_argument('--min-seconds', type=positive, default=.02)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--allow-failures', action='store_true')
    parser.add_argument('--list', action='store_true')
    args = parser.parse_args(argv)
    if args.list:
        for name, description in CASES.items():
            print(f'{name:22} {description}')
        return 0
    if args.runs < 1 or args.warmups < 0 or not 3 <= args.seed <= 20 or any(n < 1 or n > 64 for n in args.threads):
        parser.error('require runs >= 1, warmups >= 0, seed 3..20, and thread counts 1..64')
    selected = [c for c in CASES if any(fnmatch.fnmatchcase(c, p) for p in args.bench)]
    if not selected:
        parser.error('no benchmarks matched --bench')
    now = datetime.now(timezone.utc)
    output = (args.output or ROOT / 'target/benchmarks/parallel' / (now.strftime('%Y%m%dT%H%M%SZ') + '-' + uuid.uuid4().hex[:6])).resolve()
    if output.exists() and (not output.is_dir() or any(output.iterdir())):
        parser.error(f'output must be a new or empty directory: {output}')
    output.mkdir(parents=True, exist_ok=True)
    compiler = common.compiler_info('rfort', args.rfort, output, environment(1))
    report = dict(schema_version=1, created_utc=now.isoformat(), compiler=compiler,
                  machine=dict(system=platform.platform(), processor=platform.processor(), cpu_count=os.cpu_count()),
                  harness_hashes={p.name: common.digest(p) for p in (Path(__file__), HERE / 'run.py')},
                  settings={**vars(args), 'output': str(output),
                            'library_thread_limits': {k: '1' for k in ('OMP_NUM_THREADS', 'OPENBLAS_NUM_THREADS', 'MKL_NUM_THREADS', 'VECLIB_MAXIMUM_THREADS')},
                            'removed_diagnostics': ['RFORT_PARALLEL_TRACE', 'RFORT_TIME_PASSES', 'RFORT_DUMP_LOWERED']}, results=[])

    def save():
        (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        (output / 'summary.md').write_text(markdown(report), encoding='utf-8')

    save()
    if not compiler['available']:
        print(compiler.get('error', 'compiler unavailable'), file=sys.stderr)
        print(f'Report: {output / "report.json"}')
        return 1
    print('Kernel medians exclude initialization, validation and process startup. * = short timing.', flush=True)
    print(f"{'Benchmark':22} {'Size':7} {'Mode':8} {'Threads':>7} {'Status':30} {'Kernel (s)':>12} {'Process (s)':>12} {'Speedup':>9}", flush=True)
    for size in dict.fromkeys(args.size):
        for case in selected:
            rows = []
            for mode in ('serial', 'parallel'):
                built = build(case, size, mode, compiler, args, output)
                for threads in ([1] if mode == 'serial' else dict.fromkeys([1, *args.threads])):
                    row = dict(built, threads=threads, samples=[], warmups=[])
                    row['iterations_per_dispatch'] = iterations(row, args.seed)
                    rows.append(row)
                    report['results'].append(row)
                    if row['status'] == 'compiled':
                        probe_dispatch(row, args)
                    save()
            for warmup, count in ((True, args.warmups), (False, args.runs)):
                for round_index in range(count):
                    order = rows[round_index % len(rows):] + rows[:round_index % len(rows)]
                    for row in order:
                        if row['status'] == 'compiled':
                            measure(row, args, warmup)
                    save()
            for row in rows:
                summarize(row, args.min_seconds)
            speedups(rows)
            save()
            for row in rows:
                name, size_label, mode, threads, status, kernel, wall, ratio = cells(row)
                print(f'{name:22} {size_label:7} {mode:8} {threads:>7} {status:30} {kernel:>12} {wall:>12} {ratio:>9}', flush=True)
    failed = sum(r['status'] != 'passed' for r in report['results'])
    print(f"\n{len(report['results'])-failed} passed; {failed} failed.\nReport: {output/'report.json'}\nSummary: {output/'summary.md'}")
    return 0 if not failed or args.allow_failures else 1


if __name__ == '__main__':
    raise SystemExit(main())

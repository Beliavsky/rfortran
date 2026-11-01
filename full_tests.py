"""Run every tests/test_*.py with the release compiler, then cargo test --release.

Usage: python full_tests.py [-j N] [--failed] [--no-cargo] [--compiler PATH]
                            [--results DIR] [PATTERN ...]

PATTERN selects test files whose names contain it (e.g. complex picks
test_complex_*.py); --failed reruns the files not OK in the last run. Test
files run N at a time (default 4), each in its own working directory so that
module interfaces written by one file cannot collide with another's. Ctrl-C
stops the running files and writes the summary of those that finished.

Works on Windows, Linux and macOS. A progress bar shows the elapsed and
estimated remaining time, and failing test files are printed as they happen.
One summary line per test file goes to py_now.txt in the results directory
(by default rfort_full_tests in the system temporary directory), with the full
output of each failing file in its pyfail directory. At the end the files that
are not OK are listed with the baseline failures in full_tests_baseline.txt,
and the summary lines of cargo test are shown.
"""
import argparse
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
WIDTH = 110


def clock(seconds):
    seconds = int(seconds)
    return f'{seconds // 3600}:{seconds // 60 % 60:02}:{seconds % 60:02}'


def progress(done, total, start, current=''):
    filled = 30 * done // total
    elapsed = time.time() - start
    left = elapsed / done * (total - done) if done else 0
    line = (f'[{"#" * filled}{"-" * (30 - filled)}] {done}/{total} {100 * done // total}%  '
            f'{clock(elapsed)} elapsed, ~{clock(left)} left  {current}')
    sys.stdout.write('\r' + line[:WIDTH].ljust(WIDTH))
    sys.stdout.flush()


STOP = threading.Event()
PROCESSES = set()
PROCESSES_LOCK = threading.Lock()


def kill_tree(process):
    """Stop a test file and the compilers and programs it started."""
    if os.name == 'nt':
        subprocess.run(['taskkill', '/T', '/F', '/PID', str(process.pid)],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    else:
        try:
            os.killpg(process.pid, 9)
        except OSError:
            pass


def run_test(test, env, work, failures):
    """Run one test file; return (status, summary line), or None when stopped."""
    if STOP.is_set():
        return None
    work.mkdir(parents=True, exist_ok=True)
    process = subprocess.Popen([sys.executable, str(test)], cwd=work, env=env,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               text=True, errors='replace', start_new_session=os.name != 'nt')
    with PROCESSES_LOCK:
        PROCESSES.add(process)
    try:
        stdout, stderr = process.communicate(timeout=1800)
    except subprocess.TimeoutExpired:
        kill_tree(process)
        process.communicate()
        return 'TIMEOUT', f'TIMEOUT\t{test.name}'
    finally:
        with PROCESSES_LOCK:
            PROCESSES.discard(process)
    if STOP.is_set():
        return None
    text = stdout + stderr
    tail = [l for l in text.splitlines() if l.startswith(('OK', 'FAILED', 'Ran '))]
    status = 'ok' if process.returncode == 0 else 'FAIL'
    if process.returncode != 0:
        failures.mkdir(exist_ok=True)
        (failures / (test.stem + '.txt')).write_text(text, encoding='utf-8')
    return status, f'{status}\t{test.name}\t{" ".join(tail)}'


def python_tests(compiler, results, tests, jobs):
    """Run the test files, jobs at a time; return the lines of the files that are not OK."""
    env = {**os.environ, 'RFORT_TEST_COMPILER': str(compiler), 'RFORT_NO_BUILD': '1'}
    failures = results / 'pyfail'
    shutil.rmtree(failures, ignore_errors=True)
    work = results / 'work'
    shutil.rmtree(work, ignore_errors=True)
    start = time.time()
    lines = {}
    not_ok = []
    progress(0, len(tests), start)
    pool = ThreadPoolExecutor(jobs)
    running = {pool.submit(run_test, t, env, work / t.stem, failures): t for t in tests}
    pending = set(running)
    try:
        while pending:
            # Wait with a timeout: on Windows an untimed wait cannot be
            # interrupted by Ctrl-C.
            finished, pending = wait(pending, timeout=0.5, return_when=FIRST_COMPLETED)
            for future in finished:
                test = running[future]
                status, line = future.result()
                lines[test.name] = line
                if status != 'ok':
                    not_ok.append(line)
                    sys.stdout.write('\r' + f'{status}: {test.name}'.ljust(WIDTH) + '\n')
                progress(len(lines), len(tests), start, test.name)
    except KeyboardInterrupt:
        STOP.set()
        pool.shutdown(wait=False, cancel_futures=True)
        with PROCESSES_LOCK:
            for process in list(PROCESSES):
                kill_tree(process)
        print(f'\nStopped after {len(lines)} of {len(tests)} test files.')
    else:
        pool.shutdown()
    # The summary keeps file order whatever order the files finished in.
    with (results / 'py_now.txt').open('w', encoding='utf-8') as summary:
        summary.write(''.join(lines[t.name] + '\n' for t in tests if t.name in lines))
    if STOP.is_set():
        print('\n'.join(sorted(not_ok, key=lambda l: l.split('\t')[1])))
        sys.exit(130)
    print(f'\n{len(tests) - len(not_ok)} of {len(tests)} test files passed in {clock(time.time() - start)}')
    return sorted(not_ok, key=lambda l: l.split('\t')[1])


def cargo_tests(compiler):
    """Run cargo test --release with the tested compiler, showing its result
    summaries, failures and panics. Without RFORT_TEST_COMPILER the fixture
    suite would use target/debug/rfort, which may be older than the release build."""
    env = {**os.environ, 'RFORT_TEST_COMPILER': str(compiler)}
    r = subprocess.run(['cargo', 'test', '--release'], cwd=ROOT, env=env,
                       capture_output=True, text=True, errors='replace')
    for line in (r.stdout + r.stderr).splitlines():
        # The line after a panic names the failing cases of the testsuite.
        if line.startswith(('test result', 'test failures:')) or 'FAILED' in line or 'panicked' in line:
            print(line)
    return r.returncode


def main():
    exe = 'rfort.exe' if os.name == 'nt' else 'rfort'
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--compiler', type=Path, default=ROOT / 'target' / 'release' / exe,
                        help='rfort executable to test (default: the release build)')
    parser.add_argument('--results', type=Path, default=Path(tempfile.gettempdir()) / 'rfort_full_tests',
                        help='directory for py_now.txt and pyfail')
    parser.add_argument('--no-cargo', action='store_true', help='skip cargo test')
    parser.add_argument('-j', '--jobs', type=int, default=4, help='test files run at once (default 4)')
    parser.add_argument('--failed', action='store_true', help='rerun only the files not OK in the last run')
    parser.add_argument('patterns', nargs='*', help='run only test files whose names contain one of these')
    args = parser.parse_args()
    compiler = args.compiler.resolve()
    if not compiler.is_file():
        sys.exit(f'compiler not found: {compiler} (run cargo build --release first)')
    args.results.mkdir(parents=True, exist_ok=True)
    print(f'Testing {compiler}; results in {args.results}')
    tests = sorted((ROOT / 'tests').glob('test_*.py'))
    if args.patterns:
        tests = [t for t in tests if any(p in t.name for p in args.patterns)]
    if args.failed:
        previous = args.results / 'py_now.txt'
        if not previous.is_file():
            sys.exit(f'no previous results: {previous}')
        bad = {l.split('\t')[1] for l in previous.read_text(encoding='utf-8').splitlines()
               if '\t' in l and not l.startswith('ok')}
        tests = [t for t in tests if t.name in bad]
    if not tests:
        sys.exit('no test files selected')
    not_ok = python_tests(compiler, args.results, tests, max(1, args.jobs))
    print('\n=== Python files not OK:')
    print('\n'.join(not_ok) if not_ok else '(none)')
    baseline = ROOT / 'full_tests_baseline.txt'
    if baseline.is_file():
        print('\n=== Baseline failures (before these changes):')
        print(baseline.read_text(encoding='utf-8').rstrip())
    status = 0
    if not args.no_cargo:
        print('\n=== cargo test --release')
        status = cargo_tests(compiler)
    sys.exit(1 if not_ok or status else 0)


if __name__ == '__main__':
    main()

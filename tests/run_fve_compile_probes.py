"""Time compile-only fve drivers against existing, compiler-specific module files."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, default=ROOT/'target/release/rfort.exe')
    parser.add_argument('--modules', type=Path, required=True)
    parser.add_argument('--reference-modules', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--extra-module', action='append', type=Path, default=[])
    parser.add_argument('--timeout', type=float, default=120)
    parser.add_argument('sources', nargs='*', type=Path)
    args = parser.parse_args()
    base = args.output.resolve()
    base.mkdir(parents=True, exist_ok=False)
    compiler = base/args.compiler.name
    shutil.copy2(args.compiler, compiler)
    sources = args.sources or sorted(p for p in (ROOT/'tests/programs').glob('xfve_compile_*.f90') if not p.stem.endswith('_no_stats'))
    report = dict(compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(),
                  timeout=args.timeout, cases=[])
    tools = [('rfort', str(compiler), args.modules.resolve(), [])]
    if args.reference_modules:
        tools.append(('gfortran', 'gfortran', args.reference_modules.resolve(),
                      ['-O0', '-ffree-line-length-none']))
    prepared = []
    for name, executable, modules, flags in tools:
        if args.extra_module:
            workspace = base/'modules'/name
            workspace.mkdir(parents=True)
            for pattern in ('*.rmod', '*.mod'):
                for file in modules.glob(pattern): shutil.copy2(file, workspace/file.name)
            for source in args.extra_module:
                source = source.resolve()
                command = [executable, *flags, '-c', str(source), '-I', str(modules),
                           '-I', str(workspace), '-J', str(workspace),
                           '-o', str(workspace/(source.stem+'.o'))]
                with (workspace/(source.stem+'.log')).open('w') as stream:
                    subprocess.run(command, cwd=workspace, stdin=subprocess.DEVNULL,
                        stdout=stream, stderr=stream, timeout=args.timeout, check=True)
            modules = workspace
        prepared.append((name, executable, modules, flags))
    tools = prepared
    for source in sources:
        source = source.resolve()
        for name, executable, modules, flags in tools:
            work = base/source.stem/name
            work.mkdir(parents=True)
            local = work/source.name
            shutil.copy2(source, local)
            command = [executable, *flags, '-c', str(local), '-I', str(modules),
                       '-J', str(work), '-o', str(work/'probe.o')]
            log = work/'compile.log'
            start = time.perf_counter()
            code = None
            with log.open('w') as stream:
                try:
                    result = subprocess.run(command, cwd=work, stdin=subprocess.DEVNULL,
                        stdout=stream, stderr=stream, timeout=args.timeout,
                        env=dict(os.environ, RFORT_TIME_PASSES='1'))
                    code = result.returncode
                    status = 'passed' if code == 0 else 'failed'
                except subprocess.TimeoutExpired:
                    status = 'timeout'
            seconds = time.perf_counter()-start
            text = log.read_text(errors='replace')
            active = []
            for line in text.splitlines():
                if line.startswith('[pass] start '): active.append(line[13:])
                elif line.startswith('[pass] end ') and active: active.pop()
            report['cases'].append(dict(source=source.name, compiler=name,
                status=status, seconds=seconds, returncode=code, command=command,
                log=str(log), active_passes=active))
            (base/'report.json').write_text(json.dumps(report, indent=2))
            print(source.name, name, status, f'{seconds:.3f}s', '/'.join(active), flush=True)
    return int(any(c['status']=='failed' for c in report['cases']))

if __name__ == '__main__':
    raise SystemExit(main())

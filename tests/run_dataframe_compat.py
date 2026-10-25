"""Test the local DataFrame examples without modifying their source tree.

Dependency discovery is tailored to this project's free-form USE statements.
All builds and runs use fresh directories, including separate compiler module files.
"""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time

from run_ofort_compat import equivalent

MODULE = re.compile(r"^\s*module\s+(?!procedure\b|function\b|subroutine\b)(\w+)\s*$", re.I | re.M)
USE = re.compile(r"^\s*use\s*(?:,\s*(?:non_)?intrinsic\s*::\s*|::\s*)?(\w+)", re.I | re.M)
PROGRAM = re.compile(r"^\s*program\s+\w+", re.I | re.M)


def invoke(command, cwd, timeout):
    started = time.perf_counter()
    try:
        result = subprocess.run(list(map(str, command)), cwd=cwd, input='',
                                capture_output=True, text=True, errors='replace', timeout=timeout)
        return dict(command=list(map(str, command)), seconds=time.perf_counter()-started,
                    returncode=result.returncode, timed_out=False,
                    stdout=result.stdout, stderr=result.stderr)
    except subprocess.TimeoutExpired as error:
        decode = lambda value: value.decode(errors='replace') if isinstance(value, bytes) else value or ''
        return dict(command=list(map(str, command)), seconds=time.perf_counter()-started,
                    returncode=None, timed_out=True, stdout=decode(error.stdout), stderr=decode(error.stderr))


def write_report(result, report):
    root = result["source_root"]
    result['counts'] = dict(Counter(c['status'] for c in result['cases']))
    report.write_text(json.dumps(result, indent=2), encoding='utf-8')
    lines = ['# DataFrame compiler comparison', '', f'Sources: `{root}`', '',
             'Fresh isolated builds; rfort default -O2, gfortran -O0 -fcheck=all. Run timeout: '+str(result.get("run_timeout", 30))+' seconds.',
             'Successful execution alone is not proof of correctness. Random outputs are not compared across compilers.', '',
             '| Program | rfort result | gfortran | First rfort diagnostic |', '|---|---|---|---|']
    for case in result['cases']:
        error = case.get('rfort',{}).get('compile',{}).get('stderr','')
        first = next((l for l in error.splitlines() if l.lower().startswith(('error:', 'error['))), '')
        reference = case.get('gfortran', {})
        reference_status = ('ran' if reference.get('run', {}).get('returncode') == 0
                            else 'run failed' if 'run' in reference else 'compile failed')
        lines.append(f"| {case['program']} | {case['status']} | {reference_status} | {first.replace('|','/')} |")
    lines += ['', 'Counts: '+str(result['counts']), '', 'Full commands, dependencies, timings, stdout/stderr and return codes are in `report.json`.']
    report.with_suffix('.md').write_text('\n'.join(lines)+'\n',encoding='utf-8')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--compiler', type=Path, default=Path(__file__).resolve().parents[1]/'target/release'/('rfort.exe' if os.name=='nt' else 'rfort'))
    parser.add_argument('--report', type=Path, default=Path('target/dataframe-compat/report.json'))
    parser.add_argument('--compile-timeout', type=float, default=120)
    parser.add_argument('--run-timeout', type=float, default=30)
    args = parser.parse_args()
    root, compiler, report = args.root.resolve(), args.compiler.resolve(), args.report.resolve()
    if not root.is_dir() or not compiler.is_file():
        parser.error('root and compiler must exist')
    reference = shutil.which('gfortran')
    if not reference:
        parser.error('gfortran is required for the reference run')
    sources = {p.name: p.read_text(encoding='utf-8-sig') for p in sorted(root.glob('*.f90'))}
    programs = [name for name, text in sources.items() if PROGRAM.search(text) or name.startswith('x')]
    providers = defaultdict(list)
    for name, text in sources.items():
        if name not in programs:
            for module in MODULE.findall(text):
                providers[module.lower()].append(name)

    def dependencies(program):
        supplied = {m.lower() for m in MODULE.findall(sources[program])}
        seen, active, ordered = set(), set(), []
        def visit(name):
            if name in seen:
                return
            if name in active:
                raise ValueError('module dependency cycle: '+name)
            active.add(name)
            local = {m.lower() for m in MODULE.findall(sources[name])}
            for module in USE.findall(sources[name]):
                module = module.lower()
                if module in local or module in supplied or module.startswith(('iso_', 'ieee_')):
                    continue
                choices = providers.get(module, [])
                # xbug is the driver for the deliberately reduced statistics module.
                if module == 'dataframe_stats_mod':
                    choices = ['dataframe_stats_bug.f90' if program == 'xbug.f90' else 'dataframe_stats.f90']
                if len(choices) != 1:
                    raise ValueError(f'{name}: ambiguous/missing provider of {module}: {choices}')
                visit(choices[0])
            active.remove(name)
            seen.add(name)
            ordered.append(name)
        visit(program)
        return ordered

    report.parent.mkdir(parents=True, exist_ok=True)
    workspace = Path(tempfile.mkdtemp(prefix='run-', dir=report.parent))
    result = dict(run_timeout=args.run_timeout, source_root=str(root), workspace=str(workspace), compiler=str(compiler),
                  compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(),
                  versions={c: invoke([exe, '--version'], workspace, 15) for c, exe in [('rfort',compiler),('gfortran',reference)]},
                  source_sha256={n:hashlib.sha256((root/n).read_bytes()).hexdigest() for n in sources}, cases=[])

    for program in programs:
        case = dict(program=program)
        try:
            ordered = dependencies(program)
        except ValueError as error:
            case.update(status='dependency_error', error=str(error))
            result['cases'].append(case); write_report(result, report); print(program, case['status'], error, flush=True)
            continue
        case['sources'] = ordered
        stochastic = any(re.search(r'\bcall\s+random_number\b',sources[n],re.I) for n in ordered)
        case['stochastic'] = stochastic
        for label, executable in [('rfort',compiler),('gfortran',reference)]:
            directory = workspace/program.removesuffix('.f90')/label
            directory.mkdir(parents=True)
            for name in ordered:
                shutil.copy2(root/name,directory/name)
            for data in root.iterdir():
                if data.suffix.lower() in ('.csv','.dat'):
                    shutil.copy2(data,directory/data.name)
            output = directory/('program.exe' if os.name=='nt' else 'program')
            flags = [] if label=='rfort' else ['-O0','-fcheck=all']
            compiled = invoke([executable,*flags,*ordered,'-o',output],directory,args.compile_timeout)
            record = dict(compile=compiled)
            if compiled['returncode']==0 and output.is_file():
                record['run'] = invoke([output],directory,args.run_timeout)
            case[label] = record
        rc, gc = case['rfort']['compile'], case['gfortran']['compile']
        rr, gr = case['rfort'].get('run'), case['gfortran'].get('run')
        if rc['timed_out']: status='rfort_compile_timeout'
        elif rc['returncode']!=0: status='rfort_compile_failed'
        elif rr is None: status='rfort_missing_executable'
        elif rr['timed_out']: status='rfort_run_timeout'
        elif rr['returncode']!=0: status='rfort_run_failed'
        elif gc['returncode']!=0 or gr is None or gr['returncode']!=0: status='reference_failed'
        elif stochastic: status='ran_random_output_not_compared'
        elif equivalent(rr['stdout'],gr['stdout']): status='matched_reference'
        else: status='output_mismatch'
        case['status']=status
        result['cases'].append(case)
        write_report(result, report)
        print(program, status, flush=True)
    print(json.dumps(result['counts']), flush=True)
    print(report, flush=True)


if __name__ == '__main__':
    main()

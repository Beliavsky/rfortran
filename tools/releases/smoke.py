"""Exercise a distributed rfort executable from outside its source checkout."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def run(command, cwd, input=None, env=None, expected=None):
    result = subprocess.run([str(x) for x in command], cwd=cwd, input=input,
                            capture_output=True, encoding='utf-8', errors='replace',
                            timeout=120, env=env)
    if result.returncode:
        raise RuntimeError(f'{command[0]} failed ({result.returncode}):\n{result.stdout}\n{result.stderr}')
    if expected is not None and result.stdout.split() != expected.split():
        raise RuntimeError(f'Unexpected output from {command}:\n{result.stdout}\n{result.stderr}')
    return result


def smoke(binary, examples, expected_version=None):
    binary, examples = Path(binary).resolve(), Path(examples).resolve()
    results = []
    with tempfile.TemporaryDirectory(prefix='rfort release smoke ') as temporary:
        work = Path(temporary)
        # Interpretation and the REPL must not find tools or source in the checkout.
        standalone = dict(os.environ)
        standalone['PATH'] = (str(Path(os.environ['SystemRoot']) / 'System32')
                              if os.name == 'nt' else '/usr/bin:/bin')
        for name in ('RFORT_LINKER', 'RFORT_LIB_PATH', 'RFORT_CC', 'CC', 'RUSTFLAGS'):
            standalone.pop(name, None)
        version = run([binary, '--version'], work, env=standalone).stdout.strip()
        if expected_version and version.splitlines()[0] not in (f'rfort {expected_version}', f'rfort version: {expected_version}'):
            raise RuntimeError(f'Binary version {version!r} does not match {expected_version!r}')
        run([binary, '--help'], work, env=standalone)
        results.append('version/help without development tools on PATH')
        cases = [('hello.f90', None, 'Hello from rfortran!'),
                 ('arrays.f90', None, '6'), ('functions.f90', None, '42'),
                 ('input.f90', '7\n', '49')]
        # Generated objects/module caches belong in the test directory too.
        sources = work / 'examples'
        sources.mkdir()
        for name, _, _ in cases:
            shutil.copyfile(examples / name, sources / name)
        for name, input, expected in cases:
            run([binary, 'run', sources / name], work, input=input,
                env=standalone, expected=expected)
            results.append(f'interpreter: {name}')
        run([binary, 'repl'], work,
            input='integer :: n\nn=6\nn*7\n.quit\n', env=standalone, expected='42')
        results.append('REPL declaration, assignment and expression')
        for optimization in ('0', '2'):
            for name, input, expected in cases:
                executable = work / ('sample.exe' if os.name == 'nt' else 'sample')
                run([binary, '-O', optimization, 'build', sources / name,
                     '-o', executable], work)
                run([executable], work, input=input, env=standalone, expected=expected)
                results.append(f'native -O{optimization}: {name}')
    return {'binary_version': version, 'checks': results}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--examples', type=Path, default=Path(__file__).parent / 'examples')
    parser.add_argument('--version')
    args = parser.parse_args()
    print(json.dumps(smoke(args.binary, args.examples, args.version), indent=2))

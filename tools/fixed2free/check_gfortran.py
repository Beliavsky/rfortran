"""Check fixed2free against gfortran.

For each fixed-form FILE, parse the original as fixed form and the converted
file as free form with gfortran -fdump-fortran-original, and compare the parse
trees. Equal trees mean gfortran read both as the same program. The parse tree
omits FORMAT text, so for a file with FORMAT statements the format strings of
gfortran -fdump-tree-original are compared too, ignoring their blanks, since
FORMAT text joined across a continuation may keep a different number of them
without changing its meaning.

usage: python check_gfortran.py [--converter EXE] [--jobs N] [--show N] FILE_OR_DIRECTORY...

A directory contributes its .f, .for, .ftn and .f77 files, recursively.
Files that gfortran rejects in fixed form are skipped.
"""
import argparse
import concurrent.futures
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
EXTENSIONS = {'.f', '.for', '.ftn', '.f77'}
# Both forms accept legacy code and long lines, so only the form differs.
COMMON = ['-fsyntax-only', '-std=legacy', '-w', '-fdump-fortran-original', '-ffree-line-length-none']


def sources(paths):
    for path in map(Path, paths):
        if path.is_dir():
            yield from sorted(p for p in path.rglob('*') if p.suffix.lower() in EXTENSIONS and p.is_file())
        else:
            yield path


def dump(source, directory, form):
    """The parse tree of `source`, read in `directory`, or None if gfortran rejects it."""
    result = subprocess.run(['gfortran', form, *COMMON, source.name], cwd=directory,
                            capture_output=True, timeout=120)
    if result.returncode != 0:
        return None, result.stderr.decode(errors='replace')
    return normalize(result.stdout.decode(errors='replace')), ''


def normalize(tree):
    return tree.replace('\r\n', '\n')


def formats(source, directory, form):
    """The format strings that gfortran builds for `source`, without blanks."""
    work = Path(directory)
    result = subprocess.run(['gfortran', form, '-c', '-std=legacy', '-w', '-ffree-line-length-none',
                             '-fdump-tree-original', source.name, '-o', 'x.o'],
                            cwd=work, capture_output=True, timeout=120)
    if result.returncode != 0:
        return None
    dumps = list(work.glob('*.original'))
    text = dumps[0].read_text(errors='replace') if dumps else ''
    return [line.split('=', 1)[1].replace(' ', '') for line in text.splitlines() if '.format = ' in line]


def check(path, converter):
    with tempfile.TemporaryDirectory(prefix='fixed2free-') as tmp:
        fixed_dir, free_dir = Path(tmp, 'fixed'), Path(tmp, 'free')
        fixed_dir.mkdir()
        free_dir.mkdir()
        # The same file name in both, so names in the dumps agree.
        name = 'source.f'
        shutil.copy(path, fixed_dir / name)
        original, error = dump(fixed_dir / name, fixed_dir, '-ffixed-form')
        if original is None:
            return 'skip', path, error.strip().splitlines()[:3]
        converted = free_dir / name
        result = subprocess.run([converter, '-q', '-o', converted, fixed_dir / name], capture_output=True, text=True)
        if result.returncode != 0:
            return 'convert', path, [result.stderr.strip()]
        free, error = dump(converted, free_dir, '-ffree-form')
        if free is None:
            return 'reject', path, error.strip().splitlines()[:6]
        if free != original:
            diff = [line for line in difference(original, free)][:8]
            return 'differ', path, diff
        if re.search(rb'^[ \t0-9]{1,6}[ \t]*format[ \t]*\(', (fixed_dir / name).read_bytes(), re.IGNORECASE | re.MULTILINE):
            a = formats(fixed_dir / name, fixed_dir, '-ffixed-form')
            b = formats(converted, free_dir, '-ffree-form')
            if a != b:
                pairs = [f'- {x}\n    + {y}' for x, y in zip(a or [], b or []) if x != y]
                return 'differ', path, pairs[:4] or [f'{len(a or [])} formats against {len(b or [])}']
        return 'same', path, []


def difference(a, b):
    import difflib
    return (line for line in difflib.unified_diff(a.splitlines(), b.splitlines(), 'fixed', 'free', n=0, lineterm='')
            if not line.startswith(('---', '+++', '@@')))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('paths', nargs='+')
    default = HERE / 'target' / 'release' / ('fixed2free.exe' if os.name == 'nt' else 'fixed2free')
    parser.add_argument('--converter', default=str(default))
    parser.add_argument('--jobs', type=int, default=4)
    parser.add_argument('--show', type=int, default=10, help='problems to print in detail')
    args = parser.parse_args()
    args.converter = str(Path(args.converter).resolve())
    files = list(sources(args.paths))
    counts = {}
    problems = []
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        for status, path, detail in pool.map(lambda p: check(p, args.converter), files):
            counts[status] = counts.get(status, 0) + 1
            if status in ('reject', 'differ', 'convert'):
                problems.append((status, path, detail))
    for status, path, detail in problems[:args.show]:
        print(f'{status.upper()} {path}')
        for line in detail:
            print(f'    {line}')
    for status, path, _ in problems[args.show:]:
        print(f'{status.upper()} {path}')
    print(' '.join(f'{k}={v}' for k, v in sorted(counts.items())), f'files={len(files)}')
    return 1 if problems else 0


if __name__ == '__main__':
    sys.exit(main())

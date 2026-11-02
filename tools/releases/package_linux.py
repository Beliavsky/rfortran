"""Package Linux x86-64 releases and verify the downloaded archive on a fresh host."""
import argparse
import os
from pathlib import Path
import re
import struct
import subprocess

import package_unix
from package_windows import capture, dependency_licenses
from smoke import smoke

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
TARGET = 'x86_64-unknown-linux-gnu'
BASELINE = '2.35'
SYSTEM_LIBRARIES = {'libc.so.6', 'libm.so.6', 'libgcc_s.so.1', 'libpthread.so.0',
                    'libdl.so.2', 'librt.so.1', 'libutil.so.1', 'libresolv.so.2'}


def elf_dependencies(binary):
    with Path(binary).open('rb') as file:
        header = file.read(20)
    if (len(header) != 20 or header[:6] != b'\x7fELF\x02\x01'
            or struct.unpack_from('<H', header, 18)[0] != 62):
        raise ValueError('Expected a little-endian ELF64 x86-64 executable.')

    def readelf(option):
        return subprocess.run(['readelf', option, str(binary)], check=True,
                              capture_output=True, text=True,
                              env=dict(os.environ, LC_ALL='C')).stdout

    dynamic = readelf('-dW')
    needed = sorted(set(re.findall(r'\(NEEDED\).*?\[(.*?)\]', dynamic)))
    if not needed or set(needed) - SYSTEM_LIBRARIES:
        raise ValueError(f'Unexpected shared library dependencies: {needed}')
    if re.search(r'\((?:RPATH|RUNPATH)\)', dynamic):
        raise ValueError('Release executable must not contain RPATH/RUNPATH.')
    versions = set(re.findall(r'\bGLIBC_([\d.]+)\b', readelf('-VW')))
    if not versions:
        raise ValueError('No glibc symbol requirements found.')
    minimum = max(versions, key=lambda s: tuple(map(int, s.split('.'))))
    if tuple(map(int, minimum.split('.'))) > tuple(map(int, BASELINE.split('.'))):
        raise ValueError(f'Binary requires glibc {minimum}, newer than baseline {BASELINE}.')
    interpreter = re.search(r'Requesting program interpreter:\s*([^\]]+)', readelf('-lW'))
    if not interpreter or interpreter[1] != '/lib64/ld-linux-x86-64.so.2':
        raise ValueError('Expected the standard Linux x86-64 glibc ELF interpreter.')
    return dict(needed=needed, minimum_glibc=minimum, interpreter=interpreter[1])


def package(binary, output, allow_dirty=False):
    return package_unix.package(
        binary, output, allow_dirty, target=TARGET, platform_name='linux-x86_64',
        checksum_name='SHA256SUMS-linux.txt', install_name='INSTALL-linux.md',
        compatibility=f'Linux x86-64, glibc {BASELINE} or newer',
        inspect=elf_dependencies, metadata_key='elf', root=ROOT, here=HERE,
        licenses=dependency_licenses, capture=capture)


def verify(archive, report, expected_commit=None):
    return package_unix.verify(archive, report, expected_commit, target=TARGET,
                               inspect=elf_dependencies, metadata_key='elf', smoke=smoke)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    build = commands.add_parser('build')
    build.add_argument('--binary', type=Path, required=True)
    build.add_argument('--output', type=Path, default=ROOT / 'target/release-assets')
    build.add_argument('--allow-dirty', action='store_true')
    check = commands.add_parser('verify')
    check.add_argument('--archive', type=Path, required=True)
    check.add_argument('--report', type=Path, required=True)
    check.add_argument('--expected-commit')
    args = parser.parse_args()
    if args.command == 'build':
        package(args.binary, args.output, args.allow_dirty)
    else:
        verify(args.archive, args.report, args.expected_commit)

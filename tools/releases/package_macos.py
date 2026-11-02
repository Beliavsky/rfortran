"""Package and verify experimental macOS Apple Silicon release archives."""
import argparse
from pathlib import Path
import posixpath
import re
import struct
import subprocess

import package_unix
from package_windows import capture, dependency_licenses
from smoke import smoke

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
TARGET = 'aarch64-apple-darwin'
BASELINE = '14.0'


def macho_dependencies(binary):
    with Path(binary).open('rb') as file:
        header = file.read(16)
    if (len(header) != 16 or header[:4] != b'\xcf\xfa\xed\xfe'
            or struct.unpack_from('<I', header, 4)[0] != 0x0100000c
            or struct.unpack_from('<I', header, 8)[0] & 0xffffff != 0
            or struct.unpack_from('<I', header, 12)[0] != 2):
        raise ValueError('Expected a thin Mach-O arm64 executable (not Intel or arm64e).')

    def tool(*command):
        result = subprocess.run([*command, str(binary)], check=True,
                                capture_output=True, text=True)
        return result.stdout + result.stderr

    linked = tool('otool', '-L')
    needed = sorted(set(re.findall(r'^\s+(\S+) \(compatibility version', linked, re.MULTILINE)))
    if not needed or any(not posixpath.normpath(name).startswith(
            ('/usr/lib/', '/System/Library/Frameworks/')) for name in needed):
        raise ValueError(f'Unexpected non-system macOS library dependencies: {needed}')
    commands = tool('otool', '-l')
    if re.search(r'\bcmd LC_RPATH\b', commands):
        raise ValueError('Release executable must not contain LC_RPATH.')
    builds = [block for block in re.split(r'(?m)^Load command \d+\s*$', commands)
              if re.search(r'\bcmd LC_BUILD_VERSION\b', block)]
    if len(builds) != 1 or not re.search(r'\bplatform (?:1|macos)\b', builds[0], re.IGNORECASE):
        raise ValueError('Expected one macOS LC_BUILD_VERSION load command.')
    minimum = re.search(r'\bminos ([\d.]+)', builds[0])
    if not minimum:
        raise ValueError('Missing macOS minimum deployment version.')
    version = lambda value: tuple(map(int, (value.split('.') + ['0', '0'])[:3]))
    if version(minimum[1]) > version(BASELINE):
        raise ValueError(f'Binary requires macOS {minimum[1]}, newer than baseline {BASELINE}.')
    tool('codesign', '--verify', '--strict')
    signature = tool('codesign', '--display', '--verbose=2')
    if 'Signature=adhoc' not in signature:
        raise ValueError('Expected the ad-hoc code signature used by this release workflow.')
    return dict(needed=needed, minimum_macos=minimum[1], code_signature='ad-hoc; not notarized')


def package(binary, output, allow_dirty=False):
    return package_unix.package(
        binary, output, allow_dirty, target=TARGET, platform_name='macos-arm64',
        checksum_name='SHA256SUMS-macos.txt', install_name='INSTALL-macos.md',
        compatibility=f'macOS {BASELINE} or newer, Apple Silicon (arm64)',
        inspect=macho_dependencies, metadata_key='macho', root=ROOT, here=HERE,
        licenses=dependency_licenses, capture=capture)


def verify(archive, report, expected_commit=None):
    return package_unix.verify(archive, report, expected_commit, target=TARGET,
                               inspect=macho_dependencies, metadata_key='macho', smoke=smoke)


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

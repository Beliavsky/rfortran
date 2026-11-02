# rfortran @VERSION@ for macOS Apple Silicon

Experimental compiler, interpreter and REPL, forked from SirPigari/f90c.
This package targets **Apple Silicon (arm64), macOS 14 or newer**. It does not
contain an Intel executable or a universal binary.

## Extract and run

In Terminal:

```sh
tar -xzf rfortran-@VERSION@-macos-arm64.tar.gz
cd rfortran-@VERSION@-macos-arm64
./rfort --version
./rfort run examples/hello.f90
./rfort run examples/arrays.f90
printf '7\n' | ./rfort run examples/input.f90
./rfort repl
```

No Rust, Python or Fortran compiler installation is needed to interpret source.
The executable links only to macOS system libraries. BUILD.json records the
Mach-O dependencies, deployment target, source commit and executable checksum.
Keep the included license notices. To use rfort from any directory, add its
absolute installation directory to PATH.

In the REPL enter `integer :: n`, `n=6`, and `n*7` on separate lines, then
`.quit` to leave.

## Download security

This experimental executable has an **ad-hoc code signature**, not an Apple
Developer ID signature, and **is not notarized**. A valid ad-hoc signature checks
integrity but does not establish a developer identity. macOS may block a
browser-downloaded copy even though the automated archive tests pass.

Check the download against the release's SHA256SUMS.txt:

```sh
shasum -a 256 rfortran-@VERSION@-macos-arm64.tar.gz
```

If macOS blocks execution, follow Apple's instructions for allowing software
from an unidentified developer in System Settings > Privacy & Security, after
reviewing the source and checksum. Do not disable Gatekeeper globally.
See [Apple's guidance](https://support.apple.com/102445).
CI tests do not reproduce browser quarantine prompts or notarization checks.

## Native compilation

Install Apple's Command Line Tools (or a suitable Xcode installation):

```sh
xcode-select --install
```

After installation completes:

```sh
./rfort examples/hello.f90 -o hello --run
./rfort examples/functions.f90 -o functions --run
```

The driver uses `xcrun --sdk macosx clang` to compile embedded C runtime adapters
and link the generated objects. These development tools are not bundled.
`RFORT_CC` (or `CC`) can select another C compiler executable.
`rfort run file.f90` interprets; `rfort file.f90 --run` compiles and executes.

## Limitations and bug reports

The release workflow tests the extracted archive on macOS 14 and 15 Apple
Silicon runners. Smoke reports are attached to the release. This is not a
complete or production Fortran compiler; the compiler, interpreter and REPL
have different limits. See docs/feature-status.md and the matching source archive.

The combined project is GPLv3; see LICENSE, LICENSE-MIT, LICENSING.md and
THIRD-PARTY-LICENSES. Report bugs with `rfort --version`, the macOS version,
architecture, command and a small reproducer:
https://github.com/Beliavsky/rfortran/issues.

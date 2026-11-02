# rfortran @VERSION@ for Linux x86-64

Experimental compiler, interpreter and REPL, forked from SirPigari/f90c.
This archive targets 64-bit x86 Linux with glibc 2.35 or newer (Ubuntu 22.04
baseline). It does not target 32-bit x86, ARM, or musl-based systems such as
Alpine Linux. Other distributions have not necessarily been tested.

## Extract and run

```sh
tar -xzf rfortran-@VERSION@-linux-x86_64.tar.gz
cd rfortran-@VERSION@-linux-x86_64
./rfort --version
./rfort run examples/hello.f90
./rfort run examples/arrays.f90
printf '7\n' | ./rfort run examples/input.f90
./rfort repl
```

No Rust installation, Python, or Fortran compiler is needed. The executable
depends on the system glibc and GCC support libraries; its exact ELF dependencies
and glibc symbol requirements are recorded in BUILD.json. On Ubuntu/Debian,
`libc6` and `libgcc-s1` provide these libraries. Keep the included license notices.

In the REPL, enter `integer :: n`, `n=6`, and `n*7` on separate lines;
enter `.quit` to leave. To use rfort outside the extraction directory, add its
absolute directory to PATH or install the executable into a directory on PATH.

## Native compilation

Install a C compiler, linker, and C development libraries. On Ubuntu/Debian:

```sh
sudo apt-get install build-essential
./rfort examples/hello.f90 -o hello --run
./rfort examples/functions.f90 -o functions --run
```

The driver invokes `cc` to compile embedded runtime adapters and link generated
objects. `RFORT_CC` (or `CC`) can select another C compiler executable. These
tools are not bundled. `rfort run file.f90` interprets a program; `rfort file.f90
--run` compiles and executes native code. Assembly listings require an external
disassembler such as `objdump`.

## Verification and limitations

Compare the archive's `sha256sum` with the published SHA256SUMS.txt. BUILD.json
identifies the source commit and executable checksum. The release includes a
Linux smoke-test report from a separate job that downloads and tests the archive.
The matching source archive includes Cargo.lock and packaging scripts.

This is an experimental prerelease with incomplete language support. The
compiler, interpreter and REPL have different limits; see docs/feature-status.md.
The combined project is GPLv3; see LICENSE, LICENSE-MIT, LICENSING.md and
THIRD-PARTY-LICENSES. Report bugs with the version, distribution, architecture,
command and a small reproducer: https://github.com/Beliavsky/rfortran/issues.

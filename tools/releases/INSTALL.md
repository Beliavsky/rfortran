# rfortran @VERSION@ for Windows x64

This experimental prerelease contains one `rfort.exe`: native Fortran compiler,
interpreter and REPL. It is a fork of SirPigari/f90c and implements subsets of
Fortran, including features from recent standards. It is not a complete or
production compiler. See docs/feature-status.md for the current capabilities.

## Interpreter and REPL

Extract the entire archive into a directory you own. No Rust installation is
needed. In Command Prompt, from that directory:

```cmd
rfort.exe --version
rfort.exe run examples\hello.f90
rfort.exe run examples\arrays.f90
rfort.exe run examples\functions.f90
echo 7 | rfort.exe run examples\input.f90
rfort.exe repl
```

For example, in the REPL enter `integer :: n`, then `n=6`, then `n*7` on separate
lines. Enter `.quit` to leave. The compiler and interpreter have different
implementation limits; the REPL supports a smaller subset.

Add the directory containing rfort.exe to your PATH if you want to use it from
other directories. For the current Command Prompt session only:

```cmd
set "PATH=%CD%;%PATH%"
```

The executable uses a statically linked Microsoft C runtime. Interpreting source
and using the REPL do not require LLVM, Visual Studio, Python or a batch wrapper.

## Native compilation

Native compilation additionally needs LLVM with `lld-link.exe` or `ld.lld.exe`
on PATH, Visual Studio C++ Build Tools, and the Windows SDK. These external tools
are not bundled. Run from a Visual Studio x64 developer prompt if discovery
fails. `RFORT_LINKER` and `RFORT_LIB_PATH` can override linker/library discovery.

```cmd
rfort.exe examples\hello.f90 -o hello.exe
hello.exe
rfort.exe examples\functions.f90 -o functions.exe --run
```

`rfort file.f90 --run` compiles and runs native code. `rfort run file.f90` uses
the interpreter. Assembly listings require an installed objdump or dumpbin;
the optional developer-only bundled disassembler is not in this release.

## Checksums, source and bug reports

Compare the archive hash with the published SHA256SUMS.txt, for example:

```cmd
certutil -hashfile rfortran-@VERSION@-windows-x86_64.zip SHA256
```

BUILD.json identifies the version, source revision and DLL imports. The attached
release smoke-test JSON records the packaged-executable checks. PREVIEW builds
marked dirty are local development checks and must not
be published as official releases. The matching source archive contains
Cargo.lock and the release build scripts. The combined compiler is GPLv3;
see LICENSE, LICENSE-MIT, LICENSING.md and THIRD-PARTY-LICENSES.

Project and issues: https://github.com/Beliavsky/rfortran
Try the browser interpreter: https://beliavsky.github.io/rfortran/

When reporting a bug, include `rfort --version`, the command, operating system,
and a small reproducer with expected and actual output.

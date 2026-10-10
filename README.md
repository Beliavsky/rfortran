# rfortran - Fortran compiler in Rust

The compiler command is `rfort`. This project is a fork of
[SirPigari/f90c](https://github.com/SirPigari/f90c).

## Build

```sh
cargo build --release --bin rfort
```

This creates `target/release/rfort.exe` on Windows and `target/release/rfort`
on Linux/macOS. Put the executable in a directory on `PATH`. The native driver
handles compilation and linking, including multiple source files, without a
batch file or PowerShell. The Windows `rfort.bat` and `f90c.bat` entry points
forward arguments to the built executable for compatibility.

## Compile and run

```sh
rfort main.f90
rfort m1.f90 m2.f90 main.f90
rfort -o myprogram.exe main.f90
rfort -O0 --run main.f90
rfort -c main.f90 -o main.obj
rfort link main.obj -o myprogram.exe
rfort --verbose main.f90
```

On Linux/macOS, choose names such as `myprogram` and `main.o` instead.
Successful builds are silent by default; warnings and errors remain visible.
The default optimization level is `-O2`. `--verbose` shows compilation progress
and tool commands; `--quiet` suppresses that progress. `--run` executes the
linked program and propagates its failure status.

List source files in dependency order, with the main program last. They are
compiled as one source group; the compiler does not yet load module interfaces
from separately compiled object files. Existing `.obj`/`.o` files can also be
supplied for linking. Free-form `.f90`, `.f95`, `.f03`, and `.f08` inputs are
accepted. The existing language limitations still apply.

The default executable and object names come from the last source file:
`main.exe` and `main.obj` on Windows, or `main` and `main.o` on Linux/macOS.
`-o` changes the executable name, or the object name when used with `-c`.
A source group without a main program emits an object without linking;
use `-c -o file.obj` to select its object filename.

For multiple source files, the driver retains `main.f90c-sources.f90` for
diagnostics. `! Source:` comments identify the original files. Existing
wrapper-generated files are recognized and refreshed, while handwritten files
and source inputs are protected from overwrite. A single source is compiled
directly. The old `f90c-sources.ps1` helper is retained for legacy use; the
native driver does not invoke it.

## CHARACTER support

Scalar CHARACTER declarations accept constant integer length expressions,
including named parameters, LEN/LEN_TRIM of character constants, and
constant MIN/MAX and ABS calls.
`character(n)` and `character(len=n)` are supported; negative lengths become
zero. LEN of a fixed-length scalar variable does not require it to be initialized.
Assignments pad with spaces or truncate to the declared length, including
variable copies, concatenation, TRIM, and overlapping source/destination storage.
This subset uses non-NUL ASCII. Runtime-dependent lengths, CHARACTER arrays,
and CHARACTER-valued functions remain unsupported and produce diagnostics.

## File input

The current external-file I/O subset supports read-only sequential formatted
files: `OPEN(unit=20,file='data.txt',status='old',action='read')`,
`READ(20,'(a)') text`, `BACKSPACE(20)`, `REWIND(20)`, and `CLOSE(20)`.
FILE must be constant; STATUS and ACTION must be specified as shown. Units are
0 through 1023 except preconnected units 5 and 6. `(A)` reads accept one scalar
CHARACTER destination; input records must contain non-NUL ASCII. Reads consume
the entire record and handle LF/CRLF endings.
File output, external list-directed input, NEWUNIT and IOSTAT are not yet
supported. Connections are shared within a compiled source group. Native file
I/O currently requires a 64-bit target.

## Toolchains

Windows x64 is tested. Linux and macOS have explicit driver paths but still
need native execution validation; their presence is not a claim that every
compiler feature works on those platforms.

### Windows

Install LLVM (with `ld.lld.exe` or `lld-link.exe` on `PATH`), Visual Studio C++
Build Tools, and a Windows SDK. The driver uses the installed LLVM linker and
embeds the three compatibility import libraries previously used by the wrapper.
It discovers additional libraries from `LIB`, `VCToolsInstallDir`, `vswhere`,
and the Windows SDK installation. No version-specific installation paths are
compiled into the executable.

Optional environment overrides:

- `RFORT_LINKER`: linker executable path or command name. `ld.lld` receives
  `-flavor link`; `lld-link` and MSVC `link` receive their native arguments.
- `RFORT_LIB_PATH`: semicolon-separated additional library directories.

### Linux

The driver invokes the system `cc` to compile its embedded C runtime adapters
and link generated objects. Install a C compiler and development libraries.
The system toolchain supplies startup files and system library paths; bundled
Linux linker binaries and `patch.o` are no longer used.

### macOS

The driver invokes `xcrun --sdk macosx clang`, using the selected Apple Command
Line Tools/Xcode installation and SDK. It compiles the same C runtime adapters
for the host architecture. Install Apple's Command Line Tools before use.

On Linux/macOS, `RFORT_CC` (or `CC`) can select a C compiler driver. The value
must name one executable, not a shell command containing additional arguments.
Cross-target compilation and `--lto` are currently rejected explicitly.
The executable still requires an installed platform toolchain for linking.

## Other commands

```sh
rfort lex main.f90
rfort parse main.f90
rfort check main.f90
rfort build m1.f90 main.f90 -o myprogram.exe
rfort emit-obj main.f90 main.obj
rfort run main.f90
rfort --help
rfort --version
```

`run` uses the interpreter; `--run` builds and runs a native executable.
Place general options such as `--verbose` before an explicit subcommand.

## Tests

```sh
cargo build --bin rfort
cargo test --bin rfort --test testsuite -- --test-threads=1
python tests/test_driver.py
python tests/test_record_read.py
python tests/test_internal_read.py
python tests/test_write.py
python tests/test_output_implied_do.py
python tests/test_defined_operators.py
python tests/test_file_io.py
python tests/test_character_lengths.py
python tests/test_else_if.py
python tests/test_multi_forall.py
```

The Python tests use `target/debug/rfort` (with `.exe` on Windows).
`RFORT_TEST_COMPILER` selects another build; the legacy `F90C_TEST_COMPILER`
variable also works. The Rust integration suite uses the debug executable.
Tests require the platform linker/toolchain described above. The regression
runner no longer invokes `git clean`.

On Windows, after building release, also run `python tests/test_batch_sources.py`
to verify the compatibility wrappers and multi-source fixtures.

## License

The combined compiler is licensed under **[GPLv3](LICENSE)**. Original Rust
modules added in this fork and listed in [LICENSING.md](LICENSING.md) are also
available individually under the **[MIT license](LICENSE-MIT)**. Their source
files carry MIT SPDX headers. Upstream code remains subject to GPLv3.

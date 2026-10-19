# rfortran - Fortran compiler in Rust

**rfortran is an experimental Fortran compiler written in Rust**, forked from
[SirPigari/f90c](https://github.com/SirPigari/f90c). The compiler executable is
named `rfort`; the Cargo package and repository are named `rfortran`. The project
is not limited to Fortran 90, but it does not yet implement a complete Fortran
standard. Native object code is generated with Cranelift, and an interpreter is
available for development and testing.

## Current status

Development is focused on compiling small programs correctly and extending
coverage through local regressions and cases from the external ofort test suite.
Windows x64 is the tested platform. Linux and macOS have native driver paths,
but execution on those platforms still needs validation.

Implemented subsets include:

- Scalar INTEGER, REAL, DOUBLE PRECISION, COMPLEX, LOGICAL and CHARACTER values;
  named constants, supported kind selectors, arithmetic and common math intrinsics.
- Counted DO, unbounded DO and DO WHILE loops, BLOCK, ASSOCIATE, conditionals,
  SELECT CASE, and array operations including sections, explicit bounds, vector
  subscripts, WHERE, FORALL and reductions.
- Functions and subroutines, PURE/ELEMENTAL checks, INTENT, OPTIONAL/PRESENT,
  named arguments, generic interfaces, dummy procedures and procedure pointers.
- ALLOCATABLE arrays with runtime bounds, reallocation on assignment, MOVE_ALLOC
  and ALLOCATE options; pointer descriptors of ranks 1 through 15 over `TARGET`
  storage.
- Modules compiled together or separately using persistent `.rmod` interfaces;
  derived types with scalar, array, ALLOCATABLE, POINTER and nested derived-type
  components, arrays of derived types with scalar components, type-bound
  procedures and defined operators.
- List-directed output, a subset of formatted input/output, runtime format
  strings, nonadvancing output, named and scratch files.
- Initialization checks: definite reads before assignment are compile errors;
  uncertain reads are checked at runtime, with source locations and an element
  number for array errors.

These are bounded implementations, not claims of full language support. Many
array operations require statically known shapes, and procedure calls use
specialization rather than a general Fortran descriptor ABI. Important gaps
include type extension and polymorphism, GOTO and FORMAT statements, COMMON and
DATA, unformatted and direct-access I/O, most numeric edit descriptors (E, ES,
G, B, O, Z), numeric formatted input, and unbounded runtime CHARACTER storage.
[docs/feature-status.md](docs/feature-status.md) lists what is and is not
supported; [docs/language-support.md](docs/language-support.md) describes each
supported feature and its restrictions.

On the expanded ofort inventory of 440 cases with valid gfortran references,
rfort produced the reference output for 222 cases at both `-O0` and `-O2` in the
2026-10-16 run, up from 62 on 2026-10-11. This remains an early compiler, not a
replacement for an established production compiler. See
[ofort compatibility coverage](tests/ofort/README.md) for the scope, exclusions
and recorded checkpoints.

## Build

Install Rust/Cargo and the host platform's [linking toolchain](#toolchains).
Build from the repository root:

```sh
cargo build --release --bin rfort
```

This creates `target/release/rfort.exe` on Windows and `target/release/rfort`
on Linux/macOS. Put the executable in a directory on `PATH`. The native driver
handles compilation and linking, including multiple source files, without a
batch file or PowerShell.

When working on the compiler itself, the wrappers in the repository root run
the build in `target`, so you don't have to remember to rebuild:

| | Rebuilds first when sources changed | Uses the existing build |
|---|---|---|
| Windows (Command Prompt) | `rfort.bat` | `rf.bat` |
| Linux/macOS, Git Bash | `./rfort` | `./rf` |

Setting `RFORT_NO_BUILD=1` makes `rfort` skip the rebuild as well. The first
call after a source change waits for the release build. `f90c.bat` is a
compatibility entry point that calls `rfort.bat`.

For the current Windows Command Prompt session, you can add the release build
directory to PATH while in the repository root:

```bat
set "PATH=%CD%\target\release;%PATH%"
```

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

`-std=f90`, `-std=f95`, `-std=f2003`, `-std=f2008`, `-std=f2018`, and
`-std=f2023` enable partial language-revision checks before lowering. The
default remains permissive. These modes check recognized features rather than
certifying full conformance; see [standard selection](docs/usage.md#standard-selection).
Recent [Fortran 2023 additions](docs/fortran2023.md) include trimmed CHARACTER
output with `AT`, `SELECTED_LOGICAL_KIND`, and additional intrinsic kind constants.

`rfort run main.f90` interprets a program without building an executable. Run mode
is a separate implementation that is much less tested than native compilation:
few regression tests exercise it, it can be orders of magnitude slower on loops,
and it can produce results that differ from the compiled program. Prefer the
native build (`rfort --run`) when correctness matters.

Warnings (`-Wall`, `-Wunused`, `-Woptional-argument`, `-Werror`), multi-file
builds, separate compilation with `-c`, and the `.rmod` module interface files
are described in [docs/usage.md](docs/usage.md).

## How it works

rfort translates a whole source group (the files named on one command line,
plus any `.rmod` module interfaces they use) in these stages:

1. **Driver** (`src/driver.rs`). Multiple source files are combined into one
   source group, keeping a map from each line to its original file so that
   diagnostics point at the right place.
2. **Lexer** (`src/lexer.rs`). A [logos](https://crates.io/crates/logos) lexer
   produces tokens, and a second pass decides from context whether a word such
   as `associate`, `operator` or `double` is a keyword. Fortran has no reserved
   words, so the same spelling can also be a variable name.
3. **Parser** (`src/fortran.lalrpop`, `src/parser.rs`). A
   [LALRPOP](https://github.com/lalrpop/lalrpop) grammar builds the abstract
   syntax tree defined in `src/ast.rs`.
4. **Early checks and modules.** Unused-declaration and OPTIONAL-argument
   warnings run on the parsed program. `src/module_files.rs` loads the `.rmod`
   interfaces of modules that are not in the source group.
5. **Lowering** (`lower_program_body` in `src/main.rs`). A sequence of
   AST-to-AST passes rewrites advanced features into a smaller core language.
   In order, they handle USE association, dummy procedures and procedure
   pointers, named arguments, defined operators, COMPLEX values, derived types,
   ALLOCATE options, heap allocation, internal procedures, OPTIONAL arguments,
   recursion, pointers, ALLOCATABLE arrays, vector subscripts, reductions and
   array sections. Each pass is in its own source file, such as
   `src/derived.rs` or `src/procedure_arguments.rs`.
6. **Semantic checks** (`src/sema.rs`) and **initialization instrumentation**
   (`src/initialized.rs`), which adds the runtime checks for reads of undefined
   variables.
7. **Code generation.** The lowered program is translated to an intermediate
   form (`src/ir.rs`) and then to machine code with
   [Cranelift](https://cranelift.dev/) (`src/codegen/`). The driver links the
   object files with the system linker (`src/linker.rs`) and a runtime for
   formatting and I/O written in Rust and embedded in `rfort`.

`rfort run` performs stages 1–6 and then interprets the lowered program
(`src/runtime.rs`) instead of generating and linking code.

Three design choices shape what rfort supports:

- **Specialization instead of a calling convention for arrays.** Calls are
  usually compiled by making a copy of the callee for the actual arguments'
  shapes, CHARACTER lengths, OPTIONAL presence and procedure arguments. This
  avoids implementing Fortran array descriptors in every procedure, but it
  means most procedures must be compiled together with their callers, and it
  is why many features require extents known at compile time.
- **Derived types are split into their components.** A derived object `s` with
  components `x` and `a(:)` becomes separate variables `s%x` and `s%a`, and an
  array of derived type becomes one array per component. Assignment, argument
  passing and constructors are rewritten component by component.
- **Lowering to a small core.** Most features are implemented as rewrites into
  simpler statements rather than in the code generator. For example, ASSOCIATE
  substitutes the selector for each associate name, and a procedure pointer
  becomes an integer tag with a generated dispatch routine.

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
cargo test --bin rfort
cargo test --bin rfort --test testsuite -- --test-threads=1
python tests/test_driver.py
```

Each language feature has a `tests/test_*.py` script, listed in
[docs/language-support.md](docs/language-support.md). Most compare the
interpreter and native builds at `-O0` and `-O2` with gfortran. See
[docs/testing.md](docs/testing.md) for the full test list and the external
ofort and `util.f90` compatibility runners.

## Documentation

- [Feature status](docs/feature-status.md): a checklist of Fortran features
  and whether rfort supports them.
- [Language support](docs/language-support.md): supported forms,
  restrictions and tests for each feature.
- [Using rfort](docs/usage.md): warnings, multi-file builds and modules.
- [Testing](docs/testing.md): regression tests and compatibility runners.
- [Rust assessment](evaluation/RUST_ASSESSMENT.md): how Rust has affected the
  project.

## License

The combined compiler is licensed under **[GPLv3](LICENSE)**. Original Rust
modules added in this fork and listed in [LICENSING.md](LICENSING.md) are also
available individually under the **[MIT license](LICENSE-MIT)**. Their source
files carry MIT SPDX headers. Upstream code remains subject to GPLv3.

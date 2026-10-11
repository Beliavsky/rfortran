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

## Elemental subroutine setters

Scalar calls to ELEMENTAL subroutines support explicit INTENT(IN), INTENT(OUT),
and INTENT(INOUT), trailing omitted OPTIONAL arguments, and PRESENT tests.
ELEMENTAL implies PURE; PURE restrictions are checked. Supported dummy types
include INTEGER, REAL, DOUBLE PRECISION, LOGICAL, and CHARACTER. Assumed
CHARACTER dummy lengths are specialized to each scalar actual argument, with
normal assignment padding and truncation.

The four specific `set_optional_real`, `set_optional_integer`,
`set_optional_logical`, and `set_optional_character` routines from the utility
module are covered by the regression tests. This subset uses direct positional
calls; it does not add generic OPTIONAL dispatch, keyword calls, array actual
arguments, or the separate allocatable setters. Unsupported actual expressions
and mismatched types produce diagnostics. Explicit-length CHARACTER dummies
currently require matching actual lengths.

## Array bounds inquiries

`LBOUND` and `UBOUND` support statically shaped arrays and sections. A constant
`DIM` selects a scalar bound; omitting it returns a rank-one vector. Positional
arguments and `ARRAY=`, `DIM=`, and `KIND=4` keywords are accepted, as are these
keywords for `SIZE`. Nondefault result kinds and runtime dimensions are not yet
supported. Existing array-storage limits apply, including one-based declarations.
Inquiries use shape metadata and do not require initialized array elements.

## Explicit array bounds

Fixed-shape declarations accept constant INTEGER lower and upper bounds, such
as `integer :: a(0:1,-1:1), v(-2:2)`, including named-parameter expressions.
Element access, sections and section assignments translate to the compiler's
one-based storage. Dynamic supported scalar INTEGER subscripts are checked
against the original bounds before translation; invalid indices cannot wrap
and alias another element. Section endpoints/strides must currently be constant.
Omitted section endpoints use the declared bounds, including for negative
strides; write `v(2:-2:-1)` to reverse `v(-2:2)`.

LBOUND/UBOUND of whole arrays preserve their declared bounds; section bounds
start at 1. Empty dimensions report LBOUND=1 and UBOUND=0. SIZE reports
extents independently of the lower bounds. An assumed-shape dummy `x(:)` retains
its own default lower bound of 1; an explicit-shape dummy uses its declared
bounds and still requires matching extents under the current call specialization.

Runtime-dependent declaration bounds and explicit lower bounds on assumed-shape
or allocatable arrays remain unsupported. Existing limitations on array expressions and procedure arguments
still apply.

## WHERE and LOGICAL arrays

Rank-one LOGICAL arrays support constructors, whole-array and section
assignment, element access, list-directed output, and ALL/ANY reductions.
WHERE accepts single-line assignments and block constructs with plain or masked
ELSEWHERE (including `else where` and `end where` spellings). Masks are captured
before the branch executes; each assignment captures selected RHS values before
writing its destination. Inactive elemental RHS expressions are not evaluated.
Non-elemental reductions still evaluate their complete array argument.

This WHERE subset supports nested constructs and statically known multidimensional
shapes, with whole-array numeric or LOGICAL destinations. Inner masks evaluate
elemental expressions only where the enclosing mask is active; ELSEWHERE never
reactivates elements excluded by that enclosing mask. Section destinations and
CHARACTER destinations remain unsupported. Run `python tests/test_nested_where.py`
for nested mask, shape, and uninitialized-element checks.

A selected external ofort compatibility suite is described in
[tests/ofort/README.md](tests/ofort/README.md). It compares native O0/O2 output
with gfortran and records failures without changing the ofort checkout.

## Explicit DO steps

Counted loops accept `do i=first,last,step` with a constant nonzero INTEGER
step, including negative steps and named parameters. The control variable must
be a declared scalar INTEGER. Supported scalar INTEGER bounds are evaluated
once before the control variable is assigned. CYCLE advances the index, EXIT
preserves it, and empty loops retain the initial value. Increment overflow in
the compiler's 64-bit INTEGER storage reports a runtime error. Runtime-variable
steps and named loops are not yet supported.

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

## Formatted output

WRITE and PRINT accept scalar CHARACTER format expressions: literals, named
PARAMETERs, variables (including formats read from input), concatenation, and
TRIM. WRITE supports standard output (`*` or constant unit 6), with or without
an output list. Supported flat formats use A, I, F, L descriptors, repeat counts,
quoted literals, `nX` spacing, and `/` record breaks. Runtime formats are checked
for syntax, descriptor counts, and item types before the statement prints.
Uninitialized format variables use the usual initialization checks.

Formatted WRITE accepts `ADVANCE='no'` to continue on the same output line and
`ADVANCE='yes'` for the default behavior. ADVANCE must be a constant CHARACTER
expression (named parameters and concatenation are accepted). Nonadvancing
writes flush stdout, so prompts appear before a following READ. A final
nonadvancing write leaves stdout without an added newline, including at STOP.
ADVANCE cannot be used with list-directed (`*`) formats.

Output items must be scalar; existing CHARACTER procedure-argument limitations
remain. Numeric format labels, nested format groups,
format reversion, and runtime ADVANCE values are not yet supported. The existing limits of
256 for field width/precision and 10,000 for repeat counts apply.
The native formatting runtime is written in Rust, built with the compiler,
and embedded in `rfort`; no separate runtime installation is needed. Use
`rfort link` when linking emitted objects so it supplies this runtime.

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
python tests/test_array_constructors.py
python tests/test_bound_inquiries.py
python tests/test_named_formats.py
python tests/test_runtime_formats.py
python tests/test_nonadvancing_output.py
python tests/test_elemental_setters.py
python tests/test_do_steps.py
python tests/test_where.py
python tests/test_explicit_bounds.py
python tests/test_ofort_compat_runner.py
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

Numeric reductions (`SUM`, `PRODUCT`, `MINVAL`, `MAXVAL`) accept `ARRAY=`,
`DIM=`, and `MASK=` keywords, including reordered keywords. Masks may be scalar
LOGICAL values or conformable LOGICAL arrays/expressions. A constant `DIM` reduces
one axis of a fixed-shape array, preserving the other axes in order; without
DIM the whole array is reduced. Runtime DIM remains unsupported.
For a positional mask without DIM, use `MASK=`. Empty/all-false integer extrema
use the argument kind's largest/smallest representable values.
Run `python tests/test_masked_reductions.py` for interpreter/native O0/O2 checks
and comparison with gfortran when installed.

Logical reductions `COUNT`, `ANY`, and `ALL` accept `MASK=` and `DIM=` keywords.
`COUNT` supports constant `KIND=4` (default) and `KIND=8`; `ANY` and `ALL` return
LOGICAL. Whole-array and constant-DIM reductions support logical arrays,
sections, and relational array expressions. Empty inputs yield 0, false, and
true respectively. Runtime DIM remains unsupported. Run `python tests/test_logical_reductions.py` for interpreter,
native O0/O2, and gfortran comparison checks.

PURE functions with fixed-size numeric/LOGICAL `INTENT(IN)` array dummies now
accept whole-array arguments of matching shape and element kind. The compiler
copies the input elements through its scalar calling convention, so actual
arrays must be fully initialized. Different declared lower bounds are supported;
input storage is private to the function. This subset is limited to 1024 elements
per dummy and requires all actual arguments. Assumed-shape, optional/allocatable
array dummies, array expressions/sections as actuals, and non-PURE array-argument
functions are diagnosed as unsupported. Run `python tests/test_function_arrays.py`.

`HUGE` supports INTEGER kinds 1, 2, 4 and 8. Default-integer literal/constant
expressions such as `huge(0)` fold in PARAMETER initializers; executable inquiries
also accept typed scalar or array variables without reading their values.
`KIND` reports declared integer kinds correctly, including `kind(huge(i8))`.
INTEGER(16) HUGE is diagnosed rather than truncated; TINY still requires REAL.
Run `python tests/test_integer_huge.py` for interpreter/native O0/O2 and gfortran
comparison checks. General variable expressions as HUGE arguments and typed
variable inquiries inside PARAMETER initializers remain outside this subset.

Fixed-shape numeric and LOGICAL multidimensional arrays may have zero extents.
Whole-array/section operations and reductions with or without DIM handle empty inputs;
constant and runtime subscripts into an empty dimension remain errors. LOGICAL
matrices support element/section assignment, list-directed output and whole-array
COUNT/ANY/ALL. Multidimensional WHERE is also supported for whole-array destinations.
Run `python tests/test_empty_matrices.py` for empty/LOGICAL matrix checks.

`python tests/test_dim_reductions.py` checks multidimensional DIM reductions,
including masks, empty axes, nested reductions, rank-three sections, shape
diagnostics and masked uninitialized elements. Interpreter and native O0/O2
results are compared with gfortran when installed.

Numeric/LOGICAL array constructors can be used in expressions and can contain
fixed-shape array or section items, which are appended in array element order.
Items must have matching element type and kind. Constructor temporaries preserve
overlapping assignment semantics. CHARACTER and explicitly typed constructors
remain outside this subset.

`RESHAPE(SOURCE, SHAPE, PAD, ORDER)` accepts numeric/LOGICAL array expressions
and a constant SHAPE constructor, including zero extents. Optional PAD supplies
repeated elements when SOURCE is too short and must have the same type and kind.
Optional ORDER is a constant permutation of the result dimensions specifying
which dimension varies fastest during filling. Keywords may be reordered; extra
SOURCE elements are ignored. Runtime SHAPE/ORDER vectors remain unsupported.
Run `python tests/test_reshape_options.py` for padding and fill-order checks.
`MOD` and `MODULO` accept
conformable numeric array arguments and scalar expansion, preserving element
kind. Run `python tests/test_reshape_constructors.py` for interpreter/native
O0/O2 checks and gfortran comparison.

Integer `SELECT CASE` accepts comma-separated values and inclusive ranges using
`:` (for example, `case (1,3:5,9:)` or `case (:0)`). Bounds must be constant
integer expressions; duplicate/overlapping ranges are diagnosed. Literal and
arithmetic selectors work in native builds, including selectors made constant
by optimization. CHARACTER ranges remain unsupported. Run
`python tests/test_case_ranges.py` for interpreter/native O0/O2 and gfortran checks.

Separate `PARAMETER (name=value, ...)` statements are supported for explicitly
declared scalar constants. For example, `integer :: i, j` followed by
`parameter (i=5, j=i+2)` defines the same constants as a modern PARAMETER
attribute declaration. The type declaration must precede the statement; values
use the existing numeric/CHARACTER constant evaluation and conversion rules.
Module exports, host association, PURE procedures and assignment protection
are preserved. Implicitly typed constants and array names in separate PARAMETER
statements remain unsupported. Run `python tests/test_parameter_statements.py`.

CHARACTER declarations using `::` accept per-entity lengths, for example
`character(2) :: a*3, b, c*(n+1)`. An entity length overrides the shared length
without changing its neighbors. Scalar PARAMETER entities also accept `*(*)`.
Fixed-length local and host CHARACTER variables may supply `LEN(variable)` in
later CHARACTER length declarations, without reading their values. Runtime
lengths and CHARACTER array storage remain unsupported. Run
`python tests/test_entity_character_lengths.py` for interpreter/native O0/O2
and gfortran checks.

Constant `TRIM` and `LEN_TRIM` calls, including CHARACTER PARAMETER arguments,
are evaluated before code generation.

CSHIFT supports fixed-shape numeric and LOGICAL arrays and constant DIM. INTEGER
SHIFT may be scalar or an array shaped like ARRAY with DIM removed, with runtime
values in either case. Sections, array expressions, empty arrays and overlapping
assignments are supported. Runtime DIM and CHARACTER arrays remain unsupported.
Run `python tests/test_cshift.py` for interpreter/native O0/O2 checks and a
gfortran comparison.

TRANSPOSE accepts fixed-shape numeric or LOGICAL rank-two arrays, including
sections and expressions. The MATRIX keyword and PURE usage are supported.
Run `python tests/test_matrix_transforms.py` for TRANSPOSE and array-valued
CSHIFT checks in the interpreter and native O0/O2 builds, plus gfortran comparison.

EOSHIFT supports fixed-shape numeric and LOGICAL arrays with constant DIM.
SHIFT and optional BOUNDARY may be scalar or arrays shaped like ARRAY with DIM
removed. Vacated elements use BOUNDARY, or zero/false when it is omitted.
Sections, overlapping assignments and PURE usage are supported; runtime DIM
and CHARACTER arrays remain unsupported. Run `python tests/test_eoshift.py`.

SPREAD supports numeric/LOGICAL scalar and fixed-shape array sources with constant
DIM and NCOPIES. It inserts the replication dimension at DIM; zero or negative
NCOPIES produces an empty result. Sections, nested calls, constructors, overlapping
assignments and PURE usage are supported. Runtime DIM/NCOPIES and CHARACTER
sources remain unsupported. Run `python tests/test_spread.py`.

PACK supports fixed-shape numeric/LOGICAL arrays with scalar or conformable
LOGICAL masks. A supplied rank-one VECTOR sets the result length and supplies
trailing values after selected ARRAY elements. Without VECTOR, runtime masks are
supported by bounded temporary storage with a separate result length. Such results
work in list-directed printing, basic arithmetic/comparisons, whole-array assignment
to rank-one fixed-size arrays, and reductions without DIM/MASK options. Assignments
and array expressions check shape conformance at runtime. Further transformations,
constructors, array inquiries and WHERE assignments involving runtime-sized results
remain unsupported. Run `python tests/test_runtime_pack.py`.
UNPACK supports a rank-one VECTOR, fixed-shape LOGICAL MASK and scalar or
conformable FIELD of the same type/kind as VECTOR. Both intrinsics preserve
array-element order and check at runtime that VECTOR has enough elements.
CHARACTER arrays remain unsupported. Run `python tests/test_pack_unpack.py`.

MINLOC/MAXLOC support fixed-shape numeric arrays; FINDLOC also supports LOGICAL
arrays. Optional constant DIM reduces rank, or omitting DIM returns one position
per source dimension. MASK may be scalar or conformable LOGICAL; runtime scalar
BACK selects the last matching location instead of the first. Empty selections
return zeros. KIND=4 and KIND=8 are supported. FINDLOC VALUE must currently have
the same type/kind as ARRAY. Use MASK= when omitting DIM; the positional MASK-only
form, runtime DIM, CHARACTER arrays and runtime-sized PACK inputs remain outside
this subset. Run `python tests/test_locations.py`.

ISO_FORTRAN_ENV currently provides the INTEGER kind constants INT8/INT16/INT32/
INT64 (1/2/4/8) and REAL32/REAL64 (4/8). REAL128 is -1 to indicate an unsupported
real kind. USE, INTRINSIC :: ISO_FORTRAN_ENV, ordinary USE and USE :: forms support
ONLY lists, host association and re-export through user modules. Explicit
INTRINSIC selects the built-in module even if a user module has the same name;
NON_INTRINSIC requires a user module in the same source group. Other intrinsic
modules and other ISO_FORTRAN_ENV entities are not yet implemented.
Run `python tests/test_intrinsic_env.py`.

DOUBLE and PRECISION are contextual words: they form a type declaration when
paired within one logical statement, including continued lines. Otherwise they
can name variables, arrays, arguments or procedures. Run
`python tests/test_double_precision_names.py`.

Integer literals accept numeric or named kind suffixes, such as `127_1` and
`10000000000_int64`, for kinds 1, 2, 4 and 8. The compiler checks literal ranges
and preserves their kinds in supported expressions, array constructors and
procedure arguments. KIND supports numeric arithmetic expressions without
reading their operands; mixed INTEGER operands select the wider kind. A REAL
base raised to an INTEGER power retains the base kind. Exponentiation binds
more tightly than unary signs and associates to the right: `-2**2` is -4,
`(-2)**2` is 4, and `2**3**2` is 512. Nondefault-kind INTEGER PARAMETER declarations
and kind-16 integer literals remain unsupported. Run
`python tests/test_integer_literals.py`.

Main-program variables support declaration initialization from supported constant
expressions. Numeric scalars, rank-one array constructors (including constant
implied-DOs), scalar array fills, LOGICAL literals and fixed-length CHARACTER
scalars are supported. Values remain mutable; initialization runs once before
normal use, including declarations with an explicit SAVE attribute. INTEGER
initializers currently require integer constant expressions. LOGICAL expressions
beyond literals, CHARACTER arrays and additional constant array transformations
remain outside this subset. Initialized locals in procedures, modules and BLOCK
scopes are rejected until persistent storage is implemented. Run
`python tests/test_main_initialization.py`.

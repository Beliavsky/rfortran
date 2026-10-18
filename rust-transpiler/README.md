# Partial Fortran-to-Rust transpiler

This partial Rust backend translates a small subset of Fortran programs into
standalone Rust source using **the existing rfort compiler as its frontend**.
The resulting executable needs neither a Fortran compiler nor a Fortran runtime.
It makes no FFI calls and uses no external Rust crates.

All source, examples, and tests for this tool are new files under `rust-transpiler/`.
It invokes an existing rfort executable's `parse` and `check` commands. It does not
edit or rebuild compiler files, modify root Cargo files, or use the interpreter or
binding generator. Generated files and native test artifacts go under `target/`.

The pipeline is:

```text
Fortran source -> rfort parse -> serialized compiler AST -> Rust emitter
       |                                                    |
       +--------> rfort check must succeed -----------------+--> Rust source
```

There is no independent Fortran lexer, statement parser, or expression-precedence
parser in this tool. Compiler AST nodes are adapted directly; expressions are not
printed as Fortran and reparsed. The compiler's semantic/initialization checks must
succeed before Rust is written. Unsupported backend constructs are rejected before
running native-specific compiler lowerings for them.

## Run

Requires Python 3.10+ and an **existing rfort executable** to generate source, plus
rustc and its native linker to build the output. No Python packages or new Cargo
dependencies are needed. rfort is never built automatically.

Compiler selection: `--rfort PATH`, then the `RFORT` environment variable, then
this repository's `target/release/rfort.exe` (or `rfort` on other hosts), then PATH.
The frontend snapshots that executable to avoid holding the compiler build output
open while another session rebuilds it. Parse and check use the same executable
snapshot and a private, exact UTF-8 copy of the input source. Temporary frontend
files are removed after use.

For an explicitly selected build:

```cmd
python rust-transpiler\transpile.py xloop.f90 -o target\rust-transpiler\xloop.rs --rfort target\release\rfort.exe --run
```

From the repository root on Windows:

```cmd
python rust-transpiler\transpile.py xloop.f90 --run
```

`-o` is optional. Its default is `target/rust-transpiler/<source-stem>.rs`, relative
to the current working directory. For `xloop.f90`, that creates
`target/rust-transpiler/xloop.rs`; `--run` also builds and runs `xloop.exe` on
Windows (`xloop.bin` on other platforms). Supplying `-o` overrides this path.
Files with the same basename share a default output; use `-o` to distinguish them.
Omitting `--run` simply generates the Rust source at the same default path.

Supply module files in dependency order, followed by the main-program file:

```cmd
python rust-transpiler\transpile.py m1.f90 m2.f90 main.f90 --run
python rust-transpiler\transpile.py m1.f90 m2.f90 main.f90 -o target\rust-transpiler\example.rs --run
python rust-transpiler\transpile.py rust-transpiler\examples\modules\base.f90 rust-transpiler\examples\modules\derived.f90 rust-transpiler\examples\modules\main.f90 --run
```

This produces **one Rust file and one executable** for the source group. The
default output uses the final source's stem (`main.rs` above). Earlier inputs
must contain complete module definitions; the final input contains the main
program and can also define modules before it. Files are not reordered or searched
automatically. Each input is parsed independently to validate its boundaries,
then the ordered group is parsed and checked together with the existing rfort
frontend. There is no separate compilation, `.mod` loading, or Cargo dependency
generation. Existing single-file commands retain their behavior.

Backend diagnostics, located compiler diagnostics, optional source comments, and
array/format runtime errors map back to original file names and local line numbers
for source groups. Some native compiler errors have no location; those messages
are preserved without inventing a location.

The existing small examples also work, if present in your checkout:

```cmd
python rust-transpiler\transpile.py xloop.f90 -o target\rust-transpiler\xloop.rs --run
python rust-transpiler\transpile.py xmath.f90 -o target\rust-transpiler\xmath.rs --run
python rust-transpiler\transpile.py rust-transpiler\examples\formatted.f90 -o target\rust-transpiler\formatted.rs --run
```

Omit `--run` to generate Rust without compiling or executing the generated program
(rfort still runs to parse/check the source):

```cmd
python rust-transpiler\transpile.py xloop.f90 -o target\rust-transpiler\xloop.rs
rustc --edition=2021 target\rust-transpiler\xloop.rs -o target\rust-transpiler\xloop.exe
target\rust-transpiler\xloop.exe
```

The `--run` convenience option uses a 120-second compilation timeout and a
10-second program timeout. It is for trusted local examples, not public execution.
All testing so far is on x86-64 Windows; Linux/macOS have not been verified.

## Try a set of files

`run_batch.py` accepts individual files and quoted globs. It sorts each glob's
matches, removes duplicate files, continues after failures, and prints a summary.
Each selected driver is a separate translation unit. Use `--module` to prepend
one or more common module sources in dependency order to every driver:

```cmd
python rust-transpiler\run_batch.py "x*.f90" --module m1.f90 m2.f90 --run
```

Repeated `--module` options also work and can be mixed with lists. Put driver
files/globs before `--module`; following file arguments belong to the module list
until the next option.

Explicit `--module` files are excluded from driver glob matches. Their ordered
paths are included in artifact hashes and recorded in the JSON report. Each driver
is translated together with those modules into its own Rust file/executable.

```cmd
python rust-transpiler\run_batch.py "*.f90"
python rust-transpiler\run_batch.py "x*.f90" --run
python rust-transpiler\run_batch.py "x*.f90" --limit 10 --run
python rust-transpiler\run_batch.py "x*.f90" --max-failures 5 --run
python rust-transpiler\run_batch.py xloop.f90 xrandom_array.f90 --run --verbose
python rust-transpiler\run_batch.py "rust-transpiler/examples/**/*.f90" --run --report target\rust-batch.json
```

`--limit N` tries at most N drivers; N must be a positive integer. The default is
all matched drivers. Selection preserves input-pattern order and sorted matches
within each glob, after deduplication and module exclusion. Failures count toward
the limit. Unmatched inputs still report errors. The console and JSON report show
how many drivers were selected; JSON also lists drivers skipped by the limit.
Skipped files do not count as failures, and their existing artifacts are untouched.

`--max-failures N` stops after N failed drivers; N must be a positive integer.
Translation, Rust compilation, execution, output-directory failures, and timeouts
each count as one failed driver. Successful drivers and unmatched input patterns
do not consume this budget, though unmatched inputs still make the command fail.
The default is to continue after every failure. Combine it with `--limit` to cap
both attempted drivers and failures. The report is still saved when stopping early:
`drivers_attempted` and `driver_failures` give the actual counts, and
`drivers_skipped_after_failures` lists selected drivers left unprocessed.
`drivers_skipped` continues to list drivers excluded by `--limit`.

Without `--run`, it tries translation only. With `--run`, each successful
translation is compiled by rustc and executed. Statuses distinguish translation,
Rust compilation, and execution failures and timeouts. A successful run means
the program exited with status zero; output is not compared with a reference.

Generated sources and executables go into separate folders under
`target/rust-transpiler/batch`, including a path digest to distinguish identical
basenames. Each program runs in its artifact folder with standard input closed;
interactive programs receive EOF, and relative data files must be supplied there.
The default timeouts are 120 seconds for translation, 120 for compilation, and
10 for execution. Override these with `--translate-timeout`, `--compile-timeout`,
and `--run-timeout`. `--out-dir`, `--rfort`, and `--rustc` select alternate paths.
Only use `--run` for programs you intend to execute.

The default JSON report is `target/rust-transpiler/batch/report.json`; `--report`
overrides it. It includes source/artifact paths, status counts, commands, elapsed
times, exit codes, and captured stdout/stderr for every attempted stage. Console
output shows a short failure diagnostic; `--verbose` shows complete diagnostics
and program output. Reports are replaced on each invocation; artifacts from prior
runs may remain, so consult the report for current results. The runner never
executes a previous executable after a translation or compilation failure.
Exit status is zero when all selected files succeed, one for any failure
(including unmatched globs), and two for invalid command-line arguments.

## Current subset

- One free-form main program, optionally with a PROGRAM statement, with optional
  preceding module files. IMPLICIT NONE is recommended but optional; variables
  still need explicit declarations because implicit typing is not yet supported.
  Program structure and END parsing are delegated to rfort.
  Modules in the source group can contain the scalar procedures described below.
  Main-program internal procedures are not yet supported.
- Scalar `integer`, `real`, `double precision`, and `logical` declarations, with
  optional initializers and PARAMETER. Both colon and ordinary declarations use
  rfort's grammar. The explicit numeric model is
  INTEGER → `i32`, REAL → `f32`, DOUBLE PRECISION → `f64`, LOGICAL → `bool`.
  This matches the ordinary gfortran defaults used by the reference tests; it does
  not model compiler flags that change default kinds.
- Numeric kind parameters are resolved from rfort's AST, including module USE
  association, renaming, visibility, re-exports, and procedure host association.
  REAL kinds 4 and 8 map to f32 and f64; INTEGER kind 4 maps to i32. Literal
  suffixes such as `1.0_dp`, `1.0_8`, and `2_4` retain those kinds. Other numeric
  kinds are rejected explicitly. KIND supports the implemented numeric/logical
  variables, literals, and simple expressions. SELECTED_REAL_KIND and
  SELECTED_INT_KIND accept constant arguments, including keywords, and follow
  rfort's supported kind model (binary32/binary64, integer kinds 1/2/4/8).
  Selecting an integer kind does not imply the backend can declare that kind.
  Scalar constants from ISO_FORTRAN_ENV, including REAL32/REAL64 and INT32/INT64,
  can be imported and renamed. Extended precision and kind-changing compiler
  flags remain unsupported.
- Scalar assignments, arithmetic, exponentiation, numeric comparisons (including
  dotted spellings), `.true.`, `.false.`, `.and.`, `.or.`, and `.not.`.
- Block IF/ELSE/END IF and counted integer DO loops, including nested loops,
  descending loops and zero-trip loops. Bounds and stride are evaluated once.
  The emitter can represent runtime strides, but the current rfort `check` command
  rejects nonconstant strides during lowering; that diagnostic is propagated.
  A zero stride or unrepresentable final DO index raises an error.
- List-directed PRINT of scalar expressions, numeric arrays/sections, and character literals. Items are
  separated by spaces; logical output uses T/F. Formatting, field widths, floating
  point spelling, and adjacent character items are **not** an exact emulation of
  Fortran list-directed output. Character variables are absent.
- Formatted PRINT with a literal format and supported scalar expressions or
  character literals, including `print "(2i5,f10.6)", i,i**2,sqrt(real(i))`.
  This embeds the existing `src/format_spec.rs` parser/renderer, rather than
  implementing a separate formatter. It provides the same descriptor support and
  limits as that shared module, including repeats, nested groups, reversion,
  literals, integer padding, logical fields, real fields, and overflow asterisks.
  The source module is read at generation time without editing it; generated Rust
  contains the module and its MIT license notice and needs no source checkout at
  build/run time. Runtime format variables and formatted file I/O remain unsupported.
- Comments, semicolon-separated statements, and continuation between tokens.
  Continuation syntax and string decoding are handled by rfort's parser.
- Intrinsics `real`, `dble`, `int`, `abs`, `min`, `max`, `mod`, `sqrt`, `exp`, `log`,
  `log10`, `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `sinh`, `cosh`, and `tanh`.
  These math calls are positional. Conversion intrinsics accept one argument; MIN/MAX and MOD
  require arguments with matching supported types/kinds.
- Fixed-size rank-one INTEGER, REAL, and DOUBLE PRECISION arrays, with constant
  extents and the default lower bound of 1. They become Rust `Vec<T>` values.
  Whole-array assignment accepts a scalar broadcast or an array of matching known
  extent. Numeric binary arithmetic and conversions work elementwise. `sum`,
  `product`, `minval`, `maxval`, and `size` accept arrays; reductions consume
  iterators directly, including expressions such as `sum(x**2)`.
  Scalar indexing and unit-stride sections such as `x(1:nprod)` have bounds checks
  that identify the Fortran line. Initialization is tracked per element: individual
  assignments, whole-array assignments, and RANDOM_NUMBER mark their destinations
  initialized. Constant-index writes contribute to definite-assignment analysis;
  reads whose initialization depends on control flow use runtime flags. Runtime
  errors identify the first unset element read and the Fortran line. A section
  checks only its selected elements. Arrays with no preceding assignment are
  rejected before translation, and untouched backing storage is never exposed.
  Empty arrays have the usual reduction identities. Array
  constructors and array arguments to other math intrinsics
  are not yet supported.
- Numeric ALLOCATABLE arrays of ranks 1 through 15, with deferred extents in the
  declaration and default lower bounds of 1. ALLOCATE evaluates runtime extents;
  DEALLOCATE releases storage. ALLOCATED and positional SIZE(array[,dim]) query
  allocation/shape without reading uninitialized elements. Generated Rust uses
  `Option<RfArray<T>>`, with a shape vector, flat column-major storage, and per-element
  initialization flags. Whole-array scalar fills, element indexing/assignment,
  reductions, numeric conversion, array printing, and scalar-array arithmetic
  work on these arrays. Array-valued assignment to an allocatable automatically
  allocates/reallocates it to the right-hand side's shape. Allocation state,
  bounds, double allocation, and unset-element reads are checked. Reallocation
  starts with all elements unset. Total allocation size is limited to default
  INTEGER range. Allocatable scalars, custom lower bounds, sections of allocatables,
  STAT/ERRMSG/SOURCE/MOLD options, and binary operations between runtime-sized
  arrays remain outside this subset. Native frontend checks can reject some
  provably invalid allocation/inquiry cases before Rust generation.
- `call random_number(x)` for a real scalar or supported whole real array. Generated
  Rust shares one generator across calls and uses rfort's xorshift64* recurrence,
  default seed, and 24/53-bit conversion to values in `[0,1)`. Its stream differs
  from gfortran's. `random_seed` and keyword arguments are not yet supported.
- Modules containing explicitly typed scalar functions/subroutines and supported
  scalar PARAMETER constants. IMPLICIT NONE is optional in modules/procedures as
  well; explicit variable and dummy types are still required.
  USE, USE ONLY, renaming, PUBLIC/PRIVATE accessibility, cross-module
  calls, and module/USE-associated constants are supported within one source group;
  dependencies must precede consumers. Local declarations can shadow host names.
  Module constants currently require initializers that emit Rust `const` bindings.
- Procedure arguments must have explicit scalar types and INTENT. Functions accept
  INTENT(IN) arguments and return scalar values; subroutines accept IN, OUT, and
  INOUT. Input expressions must match the dummy's type/kind/rank. Output actuals
  must be distinct writable scalar variables. Generated Rust passes inputs by
  value and returns updated outputs, using a tuple for multiple outputs; it never
  reads an uninitialized caller variable to pass INTENT(OUT). Both ordinary and
  PURE procedures are supported; purity checking remains with rfort. Positional
  and keyword calls, RETURN, and supported local statements/arrays work inside
  procedures. Keyword actuals are associated with dummy names before type and
  intent checks; unknown, duplicate, and missing arguments are diagnosed.
  A scalar function used in an array arithmetic expression is evaluated once,
  rather than once per element.

For example:

```cmd
python rust-transpiler\transpile.py xrandom_array.f90 --run
```

A stable copy is also supplied as `rust-transpiler/examples/random_array.f90`.

The two-dimensional runtime allocation example also works:

```cmd
python rust-transpiler\transpile.py xalloc.f90 --run
```

The first module examples also work:

```cmd
python rust-transpiler\run_batch.py xfunc.f90 xpure_func.f90 xsub.f90 xpure_sub.f90 --run
python rust-transpiler\transpile.py rust-transpiler\examples\module_scalar.f90 --run
```

Assignments apply numeric conversions, while expressions retain their own kinds:
`7/2` is integer division even if assigned to a real variable. Default real literals
and D-exponent literals retain separate f32/f64 kinds. Exponentiation is right
associative and binds more tightly than unary minus. Integer powers with negative
exponents follow integer arithmetic. Integer operations use checked arithmetic
unless interval analysis proves the operation cannot overflow;
overflow, division by zero, and out-of-range real-to-integer conversion fail instead
of wrapping or saturating. Floating point transcendental results are tested with
tolerances, not promised to be bitwise identical to gfortran.

Unsupported backend statements and expressions produce `source.f90:line: error: ...`.
Compiler parse/check failures retain the compiler's diagnostics.
The tool also diagnoses undeclared variables, invalid scalar types, assignments to
named constants or active DO variables, and reads before definite assignment.
Scalar initialization analysis is conservative: assignments in a possibly empty DO loop
do not initialize a variable after that loop, and an IF initializes a variable
afterward only if both branches do. This can reject valid programs whose control
flow requires deeper analysis. It is not a complete Fortran conformance checker.

Fixed-size multidimensional arrays, kind selectors/suffixes, derived types,
mutable module variables, initialized/SAVE procedure locals, optional/keyword
procedure arguments, array/character procedure arguments/results, generic
interfaces, internal procedures, separately compiled modules,
pointers, complex arithmetic, READ,
formatted file I/O, ELSE IF, DO WHILE, named constructs, EXIT, CYCLE, and STOP are outside
this first subset. They are diagnosed rather than silently ignored. The mixture
examples therefore cannot yet be translated.

Intrinsic module interfaces and RANDOM_NUMBER inside procedures are also deferred.
Conservative return-path analysis can reject valid functions with more complicated
early-return control flow. Supported procedure definitions are checked even when
unused; an unsupported member of a large module still blocks translation.

## Idiomatic Rust where behavior can be preserved

The default emitter now runs a small Rust-oriented analysis over the compiler AST.
For the current `xloop.f90`, its essential output is:

```rust
fn main() {
    const N: i32 = 3;
    for i in 1..=N {
        println!("{i:5}{:5}{:10.6}", (i * i), (i as f32).sqrt());
    }
}
```

Generated files include attributes for generated-code warnings. Source-line comments
are omitted by default; add `--source-comments` when debugging a translation. No formatting module or arithmetic helper functions are included when
the generated statements do not need them.

The transformations are deliberately conditional:

- Statically evaluated default-INTEGER PARAMETERs and simple literal constants use
  uppercase Rust `const` names. Initializers requiring runtime Rust math keep `let`.
  Ordinary identifiers retain their Fortran spelling in lowercase; Rust keywords,
  helper prefixes, and collisions are renamed consistently.
- A positive unit-stride DO can become `for i in start..=limit` when the control
  variable is unobserved afterward and the final Fortran index cannot overflow.
  Bounds are still evaluated once. If the index is read later, the explicit loop
  retains its post-loop value, including for empty loops. Nonunit, descending, and
  insufficiently proven loops retain the general implementation. Nested and
  loop-carried uses are treated conservatively.
- Bounded integer expressions can use ordinary arithmetic (`i * i` for `i**2`)
  when every intermediate is proved to fit. Unknown ranges retain checked code.
- A narrow whitelist of flat `Iw` and `Fw.d` formats can become direct `println!`
  formatting if all types and field sizes are proved compatible. Integer ranges,
  finite real literal/conversion/square-root bounds, rounding carry, and negative
  zero are accounted for. The bounds are intentionally loose. Possible field
  overflow, unknown sizes, precision-zero F fields, I precision, scale/sign modes,
  literals, groups, and reversion use the unchanged shared compiler formatter.

This is not a second general format parser: the whitelist only recognizes simple
optimization patterns; rfort validates the format, and `format_spec.rs` handles the
general case. Named `square`/`root` temporaries are not invented by this version.

All analysis lives in `rust-transpiler/rust_style.py`; compiler files are unchanged.

## Tests

```cmd
python -m unittest discover -s rust-transpiler/tests -v
```

Adapter tests need only Python; validation tests also require rfort. Native tests
require rfort, rustc, and gfortran and are skipped if any is missing. They compile
generated Rust and the original Fortran
in temporary build directories and compare results, including integer division,
powers, logical expressions, scalar math, loop-index values, changed loop bounds,
negative strides, and empty loops. They also cover `xloop.f90` and `xmath.f90` when
those root examples are available. Output is compared by tokens with numeric
tolerances for list-directed output. Formatted PRINT regressions compare complete
output strings with gfortran, including leading blanks, field widths and newlines.
Source-group tests cover ordered module files, shared batch modules, dependency
errors, output naming, and original-file diagnostics. Source-map tests need only
Python; frontend tests require rfort, and group execution tests additionally need
rustc (with a gfortran reference comparison when available).

## Design and relationship to the binding generator

The reusable compiler parts are `src/lexer.rs`, `src/parser.rs`, the LALRPOP
grammar, and `src/ast.rs`, executed by `rfort parse`; and the preparation,
semantic, and initialization passes executed by `rfort check`. Compiler frontend
source is not copied. Formatted PRINT additionally reuses the existing
`src/format_spec.rs` module verbatim in generated Rust, with its copyright and MIT
permission notice. `compiler_frontend.py` reads the serialized compiler AST and adapts the
supported nodes. It constructs Python AST objects as a convenient internal
expression representation; it never calls Python's parser, eval, or exec.

The Rust emitter still maintains its own small type environment, numeric promotion,
and conservative definite-assignment checks needed for generating valid Rust.
It does **not** yet consume rfort's lowered IR or a compiler-exported symbol table:
the existing CLI exports a parsed AST and a separate checking command, not a
serialized checked IR. Compiler lowerings used by `check` can therefore impose
limits beyond what the Rust emitter could otherwise handle. Existing frontend
diagnostic limitations also carry through; the tool does not independently
revalidate complete Fortran program structure.

The optional `--source-comments` flag includes original statement line comments,
derived from compiler source locations. Diagnostic line numbers remain available
regardless of this option. A few local Rust helpers implement checked conversions and
integer powers. The AST JSON is currently an internal compiler format, not a
versioned public API; unknown AST variants produce explicit errors and the adapter
may need updating as the compiler evolves. No fallback parser is used.

Unlike `bindings/`, this tool translates the supported algorithm itself. The
binding generator leaves Fortran procedures compiled as native libraries. Longer
term, selected procedures could be translated and unsupported library routines
called through generated bindings. No automatic fallback or hybrid linking exists
in this version.

The next useful milestones are broader procedure interfaces and array support,
including array dummies/results and explicit lower bounds. Tests against gfortran should
grow with each addition before tackling mixture models or other large programs.

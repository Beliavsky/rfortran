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

## Current subset

- One free-form main program, optionally with a PROGRAM statement. IMPLICIT NONE
  is currently required. Program structure and END parsing are delegated to rfort.
  Module procedures and contained procedures are not yet supported.
- Scalar `integer`, `real`, `double precision`, and `logical` declarations, with
  optional initializers and PARAMETER. Both colon and ordinary declarations use
  rfort's grammar. The explicit numeric model is
  INTEGER → `i32`, REAL → `f32`, DOUBLE PRECISION → `f64`, LOGICAL → `bool`.
  This matches the ordinary gfortran defaults used by the reference tests; it does
  not model compiler flags that change default kinds.
- Scalar assignments, arithmetic, exponentiation, numeric comparisons (including
  dotted spellings), `.true.`, `.false.`, `.and.`, `.or.`, and `.not.`.
- Block IF/ELSE/END IF and counted integer DO loops, including nested loops,
  descending loops and zero-trip loops. Bounds and stride are evaluated once.
  The emitter can represent runtime strides, but the current rfort `check` command
  rejects nonconstant strides during lowering; that diagnostic is propagated.
  A zero stride or unrepresentable final DO index raises an error.
- List-directed PRINT of scalar expressions and character literals. Items are
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
  Calls are positional. Conversion intrinsics accept one argument; MIN/MAX and MOD
  require arguments with matching supported types/kinds.

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
Initialization analysis is conservative: assignments in a possibly empty DO loop
do not initialize a variable after that loop, and an IF initializes a variable
afterward only if both branches do. This can reject valid programs whose control
flow requires deeper analysis. It is not a complete Fortran conformance checker.

Arrays, kind selectors/suffixes, derived types, functions/subroutines, modules,
USE association, allocation, pointers, recursion, complex arithmetic, READ,
formatted file I/O, ELSE IF, DO WHILE, named constructs, EXIT, CYCLE, and STOP are outside
this first subset. They are diagnosed rather than silently ignored. The mixture
examples therefore cannot yet be translated.

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

Generated files also include source-line comments and attributes for generated-code
warnings. No formatting module or arithmetic helper functions are included when
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

Generated source includes original statement line comments, derived from compiler
source locations. A few local Rust helpers implement checked conversions and
integer powers. The AST JSON is currently an internal compiler format, not a
versioned public API; unknown AST variants produce explicit errors and the adapter
may need updating as the compiler evolves. No fallback parser is used.

Unlike `bindings/`, this tool translates the supported algorithm itself. The
binding generator leaves Fortran procedures compiled as native libraries. Longer
term, selected procedures could be translated and unsupported library routines
called through generated bindings. No automatic fallback or hybrid linking exists
in this version.

The next useful milestone is scalar procedures with explicit interfaces, followed
by arrays with explicit shape/lower-bound semantics. Tests against gfortran should
grow with each addition before tackling mixture models or other large programs.

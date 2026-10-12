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

- Scalar INTEGER, REAL, DOUBLE PRECISION, LOGICAL and CHARACTER values;
  named constants, supported kind selectors, arithmetic and common math intrinsics.
- Counted DO, unbounded DO, and DO WHILE loops, conditionals, SELECT CASE, and array operations
  including sections, explicit bounds, vector subscripts, WHERE and reductions.
- Functions and subroutines, PURE/ELEMENTAL checks, INTENT, OPTIONAL/PRESENT,
  named arguments, and supported generic procedure calls.
- Scalar and rank-1 through rank-15 pointer descriptors over fixed `TARGET`
  storage, including array views returned through subroutine output pointers.
- Modules compiled in one source group, scalar derived types, type-bound
  procedures, and defined operators, including derived-type comparisons.
- List-directed output, a subset of formatted input/output, runtime format
  strings, nonadvancing output, read-only named files, and scratch-file input/output.
- Initialization checks: definite reads before assignment are compile errors;
  uncertain reads are checked at runtime, with source locations and an element
  number for array errors.

These are bounded implementations, not claims of full language support. Many
array operations require statically known shapes, and procedure calls use
specialization rather than a general Fortran descriptor ABI. Important gaps
include runtime-dependent declaration bounds and explicit CHARACTER lengths, general
allocatable/pointer and polymorphic behavior, CHARACTER-valued functions,
separately compiled module interfaces, and broad file I/O support. The feature
notes below describe the supported forms and their restrictions.

The expanded ofort inventory refreshed on **2026-10-11** has 440 cases with valid
gfortran references. That full run recorded 62 passes, 372 compile failures, and
6 output differences at both O0 and O2. Subsequent bit-intrinsic fixes resolve
five of those output differences. Debug and release compilers now pass all 68
selected regressions (67 inventory cases plus `xradix.f90`) at both levels;
the full inventory has not been rerun after these fixes. The remaining recorded
output difference is the known REAL128 capability difference. This remains an early compiler, not a
replacement for an established production compiler. See
[ofort compatibility coverage](tests/ofort/README.md) for the scope, exclusions
and recorded results.

## Build

Install Rust/Cargo and the host platform's [linking toolchain](#toolchains).
Build from the repository root:

```sh
cargo build --release --bin rfort
```

This creates `target/release/rfort.exe` on Windows and `target/release/rfort`
on Linux/macOS. Put the executable in a directory on `PATH`. The native driver
handles compilation and linking, including multiple source files, without a
batch file or PowerShell. The Windows `rfort.bat` and `f90c.bat` entry points
forward arguments to the built executable for compatibility.

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
and INTENT(INOUT), omitted OPTIONAL arguments, named arguments, and PRESENT tests.
ELEMENTAL implies PURE unless IMPURE is specified; PURE restrictions are checked.
IMPURE ELEMENTAL functions and subroutines may perform external I/O and other
supported side effects. Either prefix order (`impure elemental` or `elemental
impure`) is accepted. Supported dummy types
include INTEGER, REAL, DOUBLE PRECISION, LOGICAL, and CHARACTER. Assumed
CHARACTER dummy lengths are specialized to each scalar actual argument, with
normal assignment padding and truncation.

The four specific `set_optional_real`, `set_optional_integer`,
`set_optional_logical`, and `set_optional_character` routines from the utility
module are covered by the regression tests. Positional and named scalar calls
are supported, including through supported generic interfaces. Numeric and
LOGICAL array actuals with constant rank-one extents are also supported for
ELEMENTAL subroutines: constructors, supported array expressions, whole arrays,
and constant sections. Scalar inputs are broadcast; writable actuals must be
arrays or sections when any actual is an array. Calls run in array element order,
preserving impure I/O and side effects. Output sections need not be initialized.
Mismatched extents and calls to impure procedures from PURE or FORALL contexts
are rejected. Dynamic extents, multidimensional whole-array subroutine actuals,
CHARACTER array actuals, and the separate allocatable setters remain outside
this subset. Unsupported actual expressions and mismatched types produce
diagnostics. Explicit-length CHARACTER dummies currently require matching actual
lengths.

`tests/126_impure_elemental.f90` and `python tests/test_impure_elemental.py`
check constructors, writable arrays and sections, OPTIONAL/named arguments,
impure elemental function calls, empty arrays, side-effect order, and diagnostics
through interpreter/native O0/O2 execution with gfortran comparisons.

## Array bounds inquiries

`LBOUND` and `UBOUND` support statically shaped arrays and sections. A constant
`DIM` selects a scalar bound; omitting it returns a rank-one vector. Positional
arguments and `ARRAY=`, `DIM=`, and `KIND=4` keywords are accepted, as are these
keywords for `SIZE`. Nondefault result kinds and runtime dimensions are not yet
supported. Constant explicit lower bounds are supported as described below;
the existing restrictions on shapes and array storage still apply.
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

## INTENT spellings

`intent(inout)` and `intent(in out)` are equivalent declaration attributes.
Both spellings use the same argument checks and propagate updates to the caller.
They are case insensitive, and `in` / `out` may be separated by a valid
free-form continuation. `intent(in)` and `intent(out)` retain their usual meanings.

## Explicit DO steps

Counted loops accept `do i=first,last,step` with a constant nonzero INTEGER
step, including negative steps and named parameters. The control variable must
be a declared scalar INTEGER. Supported scalar INTEGER bounds are evaluated
once before the control variable is assigned. CYCLE advances the index, EXIT
preserves it, and empty loops retain the initial value. Increment overflow in
the compiler's 64-bit INTEGER storage reports a runtime error. Runtime-variable
steps are not yet supported.

Unbounded `do` / `end do` loops repeat until EXIT, RETURN, or STOP transfers
control. CYCLE starts the next iteration; nested loops target the innermost
loop. A bare DO may end with a newline, a comment, or a semicolon. Continued
counted DO and DO WHILE headers retain their control clauses.

Counted, unbounded, and DO WHILE loops accept construct names, for example
`do_i: do i=1,ni` with `end do do_i` (or `enddo do_i`). `CYCLE do_i` and
`EXIT do_i` target that enclosing loop even from nested loops, BLOCK, IF, or
SELECT CASE constructs. Names are case-insensitive. Named loops require a
matching name on END DO; duplicate names and transfers to non-enclosing loops
are rejected. Unnamed CYCLE/EXIT continue to target the innermost loop.
Run `python tests/test_named_loops.py` for interpreter/native O0/O2 checks with
gfortran comparisons; `tests/137_named_loops.f90` is in the Cargo fixture suite.

## CHARACTER support

Scalar CHARACTER declarations accept constant integer length expressions,
including named parameters, LEN/LEN_TRIM of character constants, and
constant MIN/MAX and ABS calls.
`character(n)` and `character(len=n)` are supported; negative lengths become
zero. LEN of a fixed-length scalar variable does not require it to be initialized.
Assignments pad with spaces or truncate to the declared length, including
variable copies, concatenation, TRIM, and overlapping source/destination storage.
This subset uses non-NUL ASCII. Fixed rank-one CHARACTER arrays are supported
as described below. Runtime-dependent explicit lengths and CHARACTER-valued
functions remain unsupported and produce diagnostics. Bounded deferred-length
scalar assignment is described below.

## Formatted output

WRITE and PRINT accept scalar CHARACTER format expressions: literals, named
PARAMETERs, variables (including formats read from input), concatenation, and
TRIM. WRITE supports standard output (`*` or constant unit 6), with or without
an output list. Supported formats use A, I, F, L descriptors, repeat counts,
quoted literals, `nX` spacing, and `/` record breaks. Runtime formats are checked
for syntax, descriptor counts, and item types before the statement prints.
Uninitialized format variables use the usual initialization checks.

WRITE controls may be positional or named: `WRITE(*,FMT=fmt)` and
`WRITE(UNIT=6,FMT=fmt)` use the same format path. Named UNIT, FMT, and ADVANCE
controls may be reordered; positional controls must precede named controls.
Duplicate controls and missing UNIT/FMT are rejected. This also covers empty
output lists, list-directed output implied-DOs, and the supported scratch-file
output subset.
Run `python tests/test_write_controls.py` for interpreter/native O0/O2 checks
with gfortran comparisons; `tests/138_write_controls.f90` is in the Cargo suite.

Formatted WRITE accepts `ADVANCE='no'` to continue on the same output line and
`ADVANCE='yes'` for the default behavior. ADVANCE must be a constant CHARACTER
expression (named parameters and concatenation are accepted). Nonadvancing
writes flush stdout, so prompts appear before a following READ. A final
nonadvancing write leaves stdout without an added newline, including at STOP.
ADVANCE cannot be used with list-directed (`*`) formats.

Formatted PRINT/WRITE to standard output support nested implied-DO lists with
runtime INTEGER bounds and positive/negative steps. Controls are evaluated once
on entry to each implied-DO. Transfer values, including CHARACTER substrings,
are copied into a statement-local list before formatting. Memory use scales
with the list's item count and CHARACTER lengths.
Finite nested format groups and a final unlimited-repeat group such as
`(*(1x,a))` are supported. Unlimited groups require a data descriptor and repeat
until the output list ends; finite formats retain the existing exact item-count
requirement. Run `python tests/test_formatted_implied_do.py` for interpreter/native
O0/O2 checks with gfortran comparisons. The Cargo suite includes
`tests/139_formatted_implied_do.f90`.

Transfer items inside implied-DOs must resolve to supported scalar values;
existing CHARACTER procedure-argument limitations remain. Numeric format labels,
format reversion, formatted implied-DOs to file/internal units, and runtime
ADVANCE values are not yet supported. Format nesting is limited to 32 levels;
the existing limits of
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
`READ(unit,'(a)',iostat=status) text` reports zero on success, a negative value
at end of file, and a positive value on an input, encoding, or connection error.
IOSTAT must be a writable scalar INTEGER of kind 4 or 8. Failed reads leave the
CHARACTER destination undefined; reading it then triggers an initialization error.
The reader consumes invalid records so a subsequent read can continue with the
next record. These reads accept standard input as `*`, constant unit 5, or an
INTEGER unit variable whose value is 5. Input records may end without a final newline.

The format may also be a runtime scalar CHARACTER expression, including a
variable, `TRIM`, concatenation, or substring. `READ(unit,fmt,iostat=status)`
validates the format before consuming input; the supported format is `(A)`
(case-insensitive, with optional whitespace). An invalid or unsupported runtime
format gives a positive IOSTAT and leaves the destination undefined. Without
IOSTAT it stops with a runtime error. Numeric input descriptors and CHARACTER
field widths remain unsupported. Run `python tests/test_runtime_read_format.py`
for interpreter/native O0/O2 checks, including optional format dummies and
CHARACTER array-element destinations.

Empty list-directed `READ(unit,*)` and `READ(*,*)` consume exactly one record
without interpreting its contents. They support connected sequential formatted
files, scratch files, and standard input, including blank/long records and a
final record without a newline. `IOSTAT=` accepts a writable scalar INTEGER of
kind 4 or 8 and reports zero on success, a negative value at EOF, and a positive
value on an input or connection error. Without IOSTAT, failures stop with a
runtime error. Continuations, semicolon separators, and single-line IF statements
are supported. Run `python tests/test_empty_read.py` for interpreter/native
O0/O2 checks with gfortran comparisons.

READ accepts named `UNIT=` and `FMT=` controls, including reordered keywords.
`END=label` on an empty list-directed or scalar CHARACTER formatted READ branches
at EOF to a following labelled statement in the same or an enclosing statement
sequence. It can exit nested loops; other input errors still stop execution.
Backward branches, transfers into loops or IF arms, and combining END with
IOSTAT remain unsupported. Use named EXIT/CYCLE for transfers across the
region skipped by END; unqualified transfers there are diagnosed.
Run `python tests/test_read_end.py` for interpreter/native O0/O2 checks
with gfortran comparisons.

Scratch files support `OPEN(newunit=iu,status='scratch',action='readwrite')`
or an explicit UNIT, with sequential formatted access. NEWUNIT assigns a
negative unit and supports up to 1024 simultaneous scratch connections.
`WRITE(iu,'(a)')` accepts scalar CHARACTER items, producing a record for each
item through format reversion; an empty list writes an empty record. Writes
replace subsequent records at the current position. Scratch files are deleted
on close. `INQUIRE(unit=iu,name=text)` supports connected units; named files
return their OPEN path, while scratch files fill NAME with blanks.
`(A)` READ destinations may also be dynamically indexed local CHARACTER elements.

External list-directed input with transfer items, other external output formats,
named writable files, and IOSTAT on OPEN/CLOSE or positioning operations remain
unsupported.
Connections are shared within a
compiled source group. Native file I/O currently requires a 64-bit target.
Run `python tests/test_read_comments.py` for interpreter/native O0/O2 and
gfortran comparisons of these features.

Internal list-directed READ also accepts one scalar CHARACTER destination.
It handles unquoted or quoted fields, doubled quote escapes, repeated values,
null fields and slash termination, with blank padding or truncation. IOSTAT
reports success, exhausted input or malformed fields; null fields leave the
destination unchanged. Internal CHARACTER array-element destinations and
multi-record quoted strings remain unsupported.

## Numeric and logical reductions

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

## Array arguments to functions

PURE functions with fixed-size numeric/LOGICAL `INTENT(IN)` array dummies now
accept whole-array arguments of matching shape and element kind. The compiler
copies the input elements through its scalar calling convention, so actual
arrays must be fully initialized. Different declared lower bounds are supported;
input storage is private to the function. This subset is limited to 1024 elements
per dummy and requires all actual arguments. Assumed-shape, optional/allocatable
array dummies, array expressions/sections as actuals, and non-PURE array-argument
functions are diagnosed as unsupported. Run `python tests/test_function_arrays.py`.

## Integer and real inquiries

`HUGE` supports INTEGER kinds 1, 2, 4 and 8. Default-integer literal/constant
expressions such as `huge(0)` fold in PARAMETER initializers; executable inquiries
also accept typed scalar or array variables without reading their values.
`KIND` reports declared integer kinds correctly, including `kind(huge(i8))`.
INTEGER(16) HUGE is diagnosed rather than truncated; TINY still requires REAL.
Run `python tests/test_integer_huge.py` for interpreter/native O0/O2 and gfortran
comparison checks. General variable expressions as HUGE arguments and typed
variable inquiries inside PARAMETER initializers remain outside this subset.

`EPSILON`, `DIGITS`, `RADIX`, `PRECISION`, `RANGE`, `MINEXPONENT`, and
`MAXEXPONENT` support REAL kinds 4 and 8. `DIGITS`, `RADIX`, and `RANGE` also
support INTEGER kinds 1, 2, 4 and 8. EPSILON retains its argument's real kind;
the other results are default INTEGER scalars, including for array arguments.
The inquiries accept `X=` and work in PARAMETER initializers using supported
constants or previously declared variables. They determine the type without
reading variable values or evaluating array subscripts; an unset variable is
valid here. Supported arguments include variables, whole arrays, array elements,
rank-one sections, arithmetic expressions and scalar REAL/DBLE/INT conversions.
General procedure-call arguments and COMPLEX model inquiries are not implemented.
Variables and user procedures may still use these intrinsic names.

List-directed `PRINT *` with no output items emits a blank record. Large real
output, including `HUGE(0.0)` and `HUGE(0.0_8)`, no longer traps while choosing
an output representation. Values near the largest DOUBLE PRECISION number retain
enough output digits to avoid rounding beyond the finite range.
Run `python tests/test_numeric_model.py` for interpreter/native O0/O2 comparisons
with gfortran, statement boundaries, initialization behavior and invalid arguments.

## Integer bit intrinsics

Scalar `BTEST`, `IAND`, `IOR`, `IEOR`, `NOT`, `IBSET`, `IBCLR`, `IBITS`, and
`ISHFT` support INTEGER kinds 1/2/4/8 in expressions, output, conditions, PURE
procedures, and constant initialization. Named arguments are accepted. `BTEST`
returns LOGICAL; the other results retain the first argument's INTEGER kind.
`IAND`, `IOR`, and `IEOR` require both arguments to have the same kind.

Bit positions start at zero. Invalid constant positions, field lengths, and
shift counts are compile errors; dynamic values are checked at runtime.
Zero-length fields and shifts by the full bit width are supported. Initialization
checks apply to the input values. Whole-array calls, INTEGER kind 16, and
`ISHFTC` are not yet supported; scalar array elements are supported.

Run `python tests/test_bit_intrinsics.py` for interpreter/native O0/O2 checks
with gfortran comparisons. `tests/125_bit_intrinsics.f90` is the local fixture.

## Pointer association

Scalar and deferred-shape numeric or LOGICAL pointers of ranks 1 through 15
support `p => x` with fixed `TARGET` storage. The same descriptor lowering handles
every rank. Descriptors contain the storage and initialization addresses, an
association state, an origin offset, and per-dimension lower bounds, extents,
and strides. The interpreter represents addresses with checked storage handles.

Whole targets retain their lower bounds. Sections support runtime bounds,
strides, and scalar subscripts, captured at association time; scalar subscripts
reduce rank. Noncontiguous and negative-stride views use the original storage.
Pointer-to-pointer association copies the descriptor. Reassociation can change
the shape and total element count, and there is no pointer-specific 4096-element
limit. Whole-array and section assignments snapshot array right-hand sides to
preserve overlapping aliases; temporary capacity is bounded by known targets.

`NULLIFY`, `p => NULL()`, and one-argument `ASSOCIATED(p)` are supported.
Array pointers support element and whole-array assignment, section reads/writes,
`SIZE`, `SHAPE`, `LBOUND`, and `UBOUND`, including runtime DIM. Whole-array SUM,
PRODUCT, MINVAL, MAXVAL, ANY, ALL, and COUNT are supported. Reduction DIM
arguments and numeric-reduction MASK arguments remain unsupported for pointers.
Association and shape inquiries do not
read the target's values; ordinary uninitialized-value checks apply when data
is read through the pointer. Unassociated accesses and invalid dynamic
subscripts produce runtime errors. Compiler-managed pointer status starts
disassociated; standard Fortran leaves an uninitialized pointer's status
undefined, so portable code should explicitly nullify or associate it first.

Locally defined subroutines can return associations through scalar or rank-1–15
`POINTER` dummies, including OPTIONAL and named arguments. An `INTENT(OUT)`
pointer loses its association on entry without undefining the target's data.
Targets for escaping associations must be caller-owned `TARGET` variables;
associations to storage in a narrower BLOCK or call scope are rejected.

The native descriptor is currently lowered to scalar local fields. Procedure
calls still use specialization; a general descriptor calling convention and
heap-backed allocation remain future work. Early IR transformations that assume
direct variable access are disabled for procedures that access pointer storage;
Cranelift still applies the requested native optimization level.

Pointer declaration initialization, bounds/rank remapping, pointer
allocation, dynamic or allocatable targets, CHARACTER/derived-type pointers,
INTEGER(16) pointers, pointer
function arguments and results, host pointer references from unspecialized
procedures, `INTENT(IN)` pointer dummies, two-argument `ASSOCIATED`,
pointer association in PURE procedures, READ through pointers, and passing
pointers to ordinary subroutine dummies remain unsupported.

Run `python tests/test_pointer_association.py` for interpreter/native O0/O2
checks with gfortran comparisons. `tests/135_pointer_association.f90` tests the
column-pointer output pattern used by `util.f90`'s `get_data`.
Run `python tests/test_pointer_descriptors.py` for ranks 2–15, changing shapes,
nonunit bounds, strided views, output dummies, overlap, and initialization checks.
`tests/136_pointer_descriptors.f90` covers multidimensional descriptors in the
regular Cargo regression suite.

## Declarations without `::`

Intrinsic type declarations may omit `::` when they have no attributes or
initializers. Examples include `character(len=8) ampm`,
`integer(kind=4) values(8)`, and `real x,y(2)`. These forms support the same
type selectors, entity lengths, and array bounds as declarations with `::`.
Attributes such as `PARAMETER` or `INTENT`, and declaration initialization,
still require `::`.

Run `python tests/test_declarations_without_colons.py` for interpreter/native
O0/O2 checks with gfortran comparisons.

## Procedure and module endings

Subroutines and functions accept a bare `END`, an unnamed `END SUBROUTINE`
or `END FUNCTION`, or an ending with the procedure name. Modules accept
`END MODULE` with or without their name. An omitted name does not consume the
following source statement, including across comments and semicolons;
continuations within an ending statement are supported.

Run `python tests/test_unnamed_procedure_ends.py` for interpreter/native O0/O2
checks with gfortran comparisons.

## Statement labels

Executable statements may have numeric labels of one to five digits, with
values from 1 through 99999. Labels are local to a program unit, so different
procedures may reuse them; duplicates within a unit and undefined I/O branch
targets are diagnosed. `CONTINUE` is a no-op, and remains available as an
identifier outside statement context. Labelled output, input, assignments,
single-line IF statements, and supported file operations use the same behavior
as their unlabelled forms.

Labels do not yet enable GOTO, labelled DO termination, FORMAT statements, or
I/O control transfers through `ERR=` and `EOR=`. READ supports the forward
`END=` transfers described above. Unsupported controls
are rejected rather than ignored. Run `python tests/test_statement_labels.py`
for interpreter/native O0/O2 checks with gfortran comparisons.

## DIMENSION declarations and SHAPE

Intrinsic type declarations accept `DIMENSION(...)` alongside attributes such
as `INTENT`, `OPTIONAL`, `ALLOCATABLE`, `PARAMETER`, and `SAVE`. Bounds apply to
each entity; an entity's own array declarator overrides the attribute's bounds.
For example, `integer, dimension(2,3) :: a,b` declares two matrices, and
`integer, dimension(:), intent(in) :: nn` declares an assumed-shape argument.
These forms use the same supported array operations as per-entity bounds.

`SHAPE` supports fixed-size numeric/LOGICAL array variables and sections,
specialized array arguments, and supported runtime allocatable arrays. Fixed-size inquiries do
not read the array's elements, so the array need not be initialized.
The optional result `KIND` currently supports only default INTEGER (`KIND=4`);
`SOURCE=` and `KIND=` keywords are accepted. Run
`python tests/test_dimension_attribute.py` for interpreter/native O0/O2 checks
with gfortran comparisons.

## Empty arrays

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

## Constructors and RESHAPE

Numeric/LOGICAL array constructors can be used in expressions and can contain
fixed-shape array or section items, which are appended in array element order.
Items must have matching element type and kind. Constructor temporaries preserve
overlapping assignment semantics. Explicitly typed numeric/LOGICAL constructors
remain outside this subset; CHARACTER constructors use the subset described below.

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

## SELECT CASE ranges

Integer `SELECT CASE` accepts comma-separated values and inclusive ranges using
`:` (for example, `case (1,3:5,9:)` or `case (:0)`). Bounds must be constant
integer expressions; duplicate/overlapping ranges are diagnosed. Literal and
arithmetic selectors work in native builds, including selectors made constant
by optimization. CHARACTER ranges remain unsupported. Run
`python tests/test_case_ranges.py` for interpreter/native O0/O2 and gfortran checks.

## PARAMETER statements

Separate `PARAMETER (name=value, ...)` statements are supported for explicitly
declared scalar constants. For example, `integer :: i, j` followed by
`parameter (i=5, j=i+2)` defines the same constants as a modern PARAMETER
attribute declaration. The type declaration must precede the statement; values
use the existing numeric/CHARACTER constant evaluation and conversion rules.
Module exports, host association, PURE procedures and assignment protection
are preserved. Implicitly typed constants and array names in separate PARAMETER
statements remain unsupported. Run `python tests/test_parameter_statements.py`.

## CHARACTER entity lengths

CHARACTER declarations accept per-entity lengths, for example
`character(2) :: a*3, b, c*(n+1)`. An entity length overrides the shared length
without changing its neighbors. Scalar PARAMETER entities also accept `*(*)`.
Fixed-length local and host CHARACTER variables may supply `LEN(variable)` in
later CHARACTER length declarations, without reading their values. Runtime
lengths remain unsupported. Run
`python tests/test_entity_character_lengths.py` for interpreter/native O0/O2
and gfortran checks.

Constant `TRIM` and `LEN_TRIM` calls, including CHARACTER PARAMETER arguments,
are evaluated before code generation.

## Array transformations and locations

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
constructors, general array inquiries and WHERE assignments involving runtime-sized
results remain unsupported. Rank-one runtime PACK results can also be passed to
specialized INTENT(IN) array dummies, including nested PACK arguments; dummy
SIZE/LBOUND/UBOUND use the logical result extent. Run
`python tests/test_runtime_pack.py` and `python tests/test_recursive_allocatables.py`.
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

## Intrinsic modules

ISO_FORTRAN_ENV currently provides the INTEGER kind constants INT8/INT16/INT32/
INT64 (1/2/4/8) and REAL32/REAL64 (4/8). REAL128 is -1 to indicate an unsupported
real kind. USE, INTRINSIC :: ISO_FORTRAN_ENV, ordinary USE and USE :: forms support
ONLY lists, host association and re-export through user modules. Explicit
INTRINSIC selects the built-in module even if a user module has the same name;
NON_INTRINSIC requires a user module in the same source group. Other intrinsic
modules and other ISO_FORTRAN_ENV entities are not yet implemented.
Run `python tests/test_intrinsic_env.py`.

## Contextual keywords

DOUBLE and PRECISION are contextual words: they form a type declaration when
paired within one logical statement, including continued lines. Otherwise they
can name variables, arrays, arguments or procedures. Run
`python tests/test_double_precision_names.py`.

## Numeric kinds and expression semantics

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

## Declaration initialization

Main-program variables support declaration initialization from supported constant
expressions. Numeric scalars, rank-one array constructors (including constant
implied-DOs), scalar array fills, LOGICAL literals and fixed-length CHARACTER
scalars are supported. Values remain mutable; initialization runs once before
normal use, including declarations with an explicit SAVE attribute. INTEGER
initializers currently require integer constant expressions. LOGICAL expressions
beyond literals and additional constant array transformations
remain outside this subset. Initialized locals in procedures, modules and BLOCK
scopes are rejected until persistent storage is implemented. Run
`python tests/test_main_initialization.py`.

## OPTIONAL procedures

Local OPTIONAL functions and subroutines can be specialized for positional and
named calls, including omitted arguments before later named arguments and
forwarding an absent optional dummy to another optional dummy. PRESENT
and simple logical combinations of presence tests select the applicable branches;
a missing required argument or an unguarded absent-dummy reference is diagnosed.
Optional functions currently require explicit INTENT(IN) on their arguments.
Recursive calls reuse the specialization for each argument-presence combination.
A separate-object optional-argument ABI remains unsupported. Named calls require
a definition in the source group.

Scalar CHARACTER subroutine arguments support fixed and assumed lengths,
including OPTIONAL inputs and writable variables. Assumed lengths follow each
actual argument; LEN, empty strings and literal/concatenated inputs work. Fixed
lengths currently require an exact match. Expressions require INTENT(IN), while
writable arguments must be variables. Explicit RETURN leaves an expanded
subroutine, including from nested loops, and execution resumes in its caller.
Fixed-length rank-one CHARACTER array subroutine arguments also use call-site
specialization, including assumed-length dummies. Assumed-length function
dummies remain outside this subset.
Run `python tests/test_optional_procedures.py`.

## Internal procedures and host association

Functions and subroutines accept CONTAINS with internal functions/subroutines.
Internal procedures can read and update INTEGER(4/8), REAL(4/8), and LOGICAL
variables explicitly declared in the enclosing procedure, including its dummy arguments
and array elements of ranks 1 through 15. Array accesses retain lower bounds,
check subscripts, and share the original storage and initialization flags.
SIZE, SHAPE, LBOUND, and UBOUND inquiries are supported; DIM must be constant.

Recursive helpers with scalar input arguments forward their host environment,
so a quick-sort helper can modify the enclosing procedure's arrays without
copying them. Sibling calls share that environment, local declarations shadow
host variables, and identical helper names in separate hosts remain distinct.
Reads of undefined host values retain runtime checks; writes to host INTENT(IN)
variables are rejected. The native backend passes borrowed storage addresses;
the interpreter uses handles identifying the owning call frame.

Capturing CHARACTER, POINTER, ALLOCATABLE, or OPTIONAL host objects, whole-array
host values/assignments, sections, READ/I/O definitions through host objects,
and forwarding captured objects directly to ordinary subroutine dummies or
internal writable dummy arguments remain unsupported. Internal function results
must have explicit types. Internal procedures cannot yet
contain further internal procedures. Procedure values/escaping closures are
unsupported; recursive array/output dummy arguments retain the limits below.
Run `python tests/test_internal_procedures.py` for interpreter/native O0/O2 checks
with gfortran comparisons. The Cargo suite includes
`tests/140_internal_procedures.f90`.

## Recursive procedures and bounded allocatable outputs

RECURSIVE is accepted on functions and subroutines, including PURE RECURSIVE
and RECURSIVE PURE prefixes. Scalar recursive functions can reuse OPTIONAL
specializations. Array subroutines still use call-site expansion: recursion must
terminate during specialization, as in `xpure_recursive.f90`, whose recursive
call omits the optional bounds and selects the copy branch. General recursive
array calls remain unsupported and produce a diagnostic.

Local rank-one numeric/LOGICAL or fixed-length CHARACTER ALLOCATABLE INTENT(OUT) dummies can associate with
main-program allocatable arrays. Storage capacity is inferred from bounded array
inputs across the source group's call sites, or from fixed local arrays when
no array input is available; allocation state and the current
extent remain runtime values. INTENT(OUT) deallocates the previous output on entry.
ALLOCATE accepts runtime extents within that capacity, including zero, followed
by conformable whole-array assignment or scalar filling. ALLOCATED, SIZE, LBOUND,
UBOUND, reductions and list-directed output use the current allocation and extent.
Repeated allocation, reads of unallocated arrays and allocations exceeding the
inferred capacity report runtime errors. Element initialization checks still apply.
CHARACTER outputs support ALLOCATED, SIZE, element access, scalar filling and
conformable whole-array or constant-stride section copying, including padding
and truncation when element lengths differ. CHARACTER reductions and whole-array
output of runtime-sized CHARACTER arrays remain unsupported.

This does not provide general heap allocation, automatic reallocation on array
assignment, dynamic input declaration bounds, multidimensional allocatable outputs,
sections of runtime-sized dummies/allocatable arrays, or a descriptor ABI.
Growing array call cycles cannot supply a finite storage
bound and are diagnosed. Run `python tests/test_recursive_allocatables.py` for
interpreter/native O0/O2 checks and comparison with gfortran.

DEALLOCATE accepts lists of whole bounded allocatable arrays and
deferred-length scalar CHARACTER locals, including in single-line IF statements.
It clears ALLOCATED and initialization state, permits subsequent allocation or
CHARACTER assignment, and reports an error when an object is already unallocated.
Local rank-one arrays explicitly deallocated in the source reserve bounded storage
from their ALLOCATE shapes, which must be known during call-site specialization;
different extents and zero-sized allocations are supported within that capacity.
Supported allocatable INTENT(INOUT) dummies share allocation state with the caller;
INTENT(IN) objects and nonlocal objects in PURE procedures cannot be deallocated.
Pointers, derived-type objects, and STAT/ERRMSG options
remain outside this DEALLOCATE subset. Run `python tests/test_deallocate.py` for
interpreter/native O0/O2 checks and comparison with gfortran.

Local numeric/LOGICAL multidimensional allocatables explicitly deallocated in
the source can also use runtime allocation dimensions with proven finite bounds.
For example, choosing `(3,4)` or `(10,20)` in an IF/ELSE reserves capacity for 200
elements while each ALLOCATE sets the active dimensions and element count.
SIZE (including constant DIM), SHAPE, LBOUND, UBOUND, ALLOCATED, whole-array filling,
arithmetic, list-directed output, full-array reductions, and RANDOM_NUMBER use the
current allocation. Element indexing uses the current column-major strides and
checks each subscript against its dimension. Array operations check each logical
dimension, including when different shapes have the same total element count.
Deallocation and reallocation reset element initialization state; empty allocations
can be queried and reduced without reading storage.

This continues to use bounded local storage. Input-dependent bounds and
loop-carried growth without a finite proof are diagnosed; general heap allocation
is not implemented. Runtime-sized multidimensional sections, procedure arguments,
array transformations, masked/DIM reductions, mixing fixed and runtime-sized
multidimensional array expressions, and explicit non-unit allocation lower bounds
remain unsupported. Run `python tests/test_runtime_allocatables.py` for
interpreter/native O0/O2 checks and comparisons with gfortran for deterministic cases.

## CHARACTER comparisons and function arguments

Scalar CHARACTER comparisons support all six relational operators with blank
padding of the shorter operand. Fixed-length variables, ASCII literals,
concatenations, TRIM and zero-length strings are supported. Comparison captures
in DO WHILE conditions are reevaluated on every iteration, including after CYCLE.
Run `python tests/test_character_comparisons.py`.

Local functions can receive fixed-length, read-only scalar CHARACTER arguments.
Actuals may be variables, literals or fixed-length concatenations; they must be
at least as long as the dummy, and longer actuals contribute their leading
characters. Function-to-function forwarding is supported. Writable CHARACTER
function dummies, runtime-length actuals and assumed-length dummies remain
unsupported.

## Scalar substrings

Scalar CHARACTER variables support substring reads with constant or runtime
scalar INTEGER bounds, including omitted endpoints and empty ranges. String
literals also accept substring syntax, such as `'0123456789'(first:last)`.
Runtime bounds are evaluated once and checked against the current string length
before reading its storage. Substring assignments still require constant bounds.
Overlapping
assignments capture the right-hand side before updating the destination;
shorter values are blank-padded and longer ones truncated. LEN of a substring
uses its declared bounds without reading the parent string. Partial writes
currently require an initialized parent string; initialization tracking is not
yet per character. Dynamic assignment bounds, substrings of
PARAMETER strings, READ destinations and substring actuals to subroutines remain
unsupported. Run `python tests/test_scalar_substrings.py`.

### Deferred-length scalar CHARACTER

Main-program and BLOCK locals declared `character(len=:), allocatable` support
assignment from literals, supported scalar CHARACTER expressions and substrings.
Assignment adjusts the logical length, including shrinking to zero, while
preserving overlapping source values. LEN, LEN_TRIM, ALLOCATED, concatenation,
comparisons and supported output operations use the current value and length.
For example:

```fortran
implicit none
character(len=:), allocatable :: s
integer :: first=3, last=7
s = '0123456789'(first:last)
print '(a)', s  ! 23456
end
```

This subset reserves bounded storage inferred from the possible assignment
sources; it does not implement general heap allocation. An unknown maximum
length or growing self-concatenation such as `s=s//'x'` is diagnosed. Deferred
CHARACTER arrays, procedure/module storage and host association, explicit
ALLOCATE, and READ into a deferred-length destination remain
unsupported. Other intrinsic/procedure calls with runtime-length CHARACTER
arguments are diagnosed unless implemented for this subset (such as TRIM).
Runtime substring conditions are reevaluated on each loop iteration,
including after CYCLE. The scalar substring tests compare interpreter and native
O0/O2 results with gfortran when available.

## CHARACTER arrays

Fixed-size rank-one CHARACTER arrays support up to 1024 elements in local
storage. Supported operations include literal constructors, main-program
declaration initialization, scalar fills, whole-array copies, constant element
indices, runtime element indexing, constant sections (including strides), and
elementwise substring assignment.
All source values are captured before overlapping destinations are written.
Array comparisons support scalar broadcasting and ALL/ANY without DIM;
list-directed PRINT, whole-array SIZE and LEN are supported. Initialization is
tracked per element, with diagnostics naming the original array and its storage
element number. Partial substring writes still require initialized elements.

Dynamic accesses check bounds and initialize only the selected element; DO WHILE
conditions with dynamic CHARACTER indexing are evaluated on every iteration.
Local CHARACTER array subroutines can specialize fixed/assumed-length dummies
to caller storage, including bounded allocatable outputs.
Higher ranks, module CHARACTER array storage, CHARACTER
array PARAMETERs and the other array transformational intrinsics remain outside
this subset. Untyped constructors require equal-length CHARACTER literals
(including substituted constants). Typed `[CHARACTER(LEN=n) :: ...]` constructors
accept constant CHARACTER elements of different lengths and a constant length;
elements are padded or truncated to that length before assignment. Run
`python tests/test_character_arrays.py`.

## Vector subscripts

Local fixed rank-one numeric and LOGICAL arrays support INTEGER vector subscripts with
up to 1024 selected elements. Selectors may be fixed-size INTEGER arrays,
constructors, constant implied-DOs, sections, supported arithmetic, ABS/MOD/MODULO
or default INT conversions. Reads preserve order and repetitions. Assignments
capture all indices and right-hand values before writing, including scalar fills,
overlapping selections and an array used as its own index vector. Nondefault
lower bounds and INTEGER kinds 1/2/4/8 are supported.

Constant invalid indices and duplicate assignment indices are compile errors;
dynamic indices are checked against the original bounds before any writes, and
dynamic duplicate destinations cause a runtime error. Duplicate indices remain
valid for reads. SIZE, LBOUND, UBOUND and KIND of supported selections use shape
and type information without reading the array or index values. Only selected
source elements must be initialized. Loop-condition captures are reevaluated on
every iteration, including after CYCLE. Run `python tests/test_vector_subscripts.py`.

Higher-rank and CHARACTER selections, runtime-sized index vectors, READ
destinations, vector actuals to subroutines and vector selections in WHERE
assignments remain unsupported. Other vector expressions are rejected when they
reach scalar array-index lowering, instead of silently using their first element.

## Named arguments

Calls to procedures defined in the source group support named arguments,
including reordered keywords and positional arguments followed by keywords.
Omitted OPTIONAL arguments can occur before later named arguments, and absent
OPTIONAL values can be forwarded by name. Generic calls associate keywords with
each candidate's dummy names before matching types. Named scalar, supported
array/CHARACTER and type-bound procedure calls use the existing procedure
lowering. Common math intrinsics and RANDOM_NUMBER(HARVEST=...) also accept names;
existing intrinsic keyword support remains available.

Unknown keywords, duplicate arguments, positional arguments after keywords and
missing required arguments are compile errors. Named procedure calls currently
require a known definition in the source group and unique procedure names within
the compilation; external interface-only declarations and procedure pointers
remain unsupported. Run `python tests/test_named_arguments.py`.

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
python tests/test_runtime_read_format.py
python tests/test_named_loops.py
python tests/test_write_controls.py
python tests/test_formatted_implied_do.py
python tests/test_internal_procedures.py
python tests/test_empty_read.py
python tests/test_read_end.py
python tests/test_statement_labels.py
python tests/test_unnamed_procedure_ends.py
python tests/test_declarations_without_colons.py
python tests/test_pointer_association.py
python tests/test_pointer_descriptors.py
python tests/test_recursive_allocatables.py
python tests/test_deallocate.py
python tests/test_runtime_allocatables.py
python tests/test_dimension_attribute.py
python tests/test_internal_read.py
python tests/test_write.py
python tests/test_output_implied_do.py
python tests/test_defined_operators.py
python tests/test_file_io.py
python tests/test_read_comments.py
python tests/test_character_lengths.py
python tests/test_else_if.py
python tests/test_multi_forall.py
python tests/test_array_constructors.py
python tests/test_bound_inquiries.py
python tests/test_named_formats.py
python tests/test_runtime_formats.py
python tests/test_nonadvancing_output.py
python tests/test_elemental_setters.py
python tests/test_impure_elemental.py
python tests/test_do_steps.py
python tests/test_where.py
python tests/test_explicit_bounds.py
python tests/test_nested_where.py
python tests/test_masked_reductions.py
python tests/test_dim_reductions.py
python tests/test_optional_procedures.py
python tests/test_character_arrays.py
python tests/test_scalar_substrings.py
python tests/test_vector_subscripts.py
python tests/test_named_arguments.py
python tests/test_ofort_compat_runner.py
python tests/test_numeric_model.py
python tests/test_bit_intrinsics.py
```

The Python tests use `target/debug/rfort` (with `.exe` on Windows).
`RFORT_TEST_COMPILER` selects another build; the legacy `F90C_TEST_COMPILER`
variable also works. The Rust integration suite uses the debug executable.
Tests require the platform linker/toolchain described above. The regression
runner no longer invokes `git clean`.

The Rust integration suite checks the numbered native fixtures against
[tests/expectations.json](tests/expectations.json). The commands above include
focused Python checks for the driver and language features; additional
`tests/test_*.py` scripts cover other implemented subsets. Many compare interpreter
execution with native builds at O0 and O2, and also compare with gfortran when it
is available. Python is required for these scripts; gfortran is required for the
external compatibility runner.

On Windows, after building release, also run `python tests/test_batch_sources.py`
to verify the compatibility wrappers and multi-source fixtures.

### External ofort fixtures

After building release, run the quick 18-case sample or the broader inventory:

```bat
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --report target/ofort-compat.json
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --discover --jobs 4 --report target/ofort-expanded.json
```

Replace `C:\c\ofort` with your ofort checkout. These commands compile standalone
fixtures with rfort and gfortran; they do not run the full ofort Python suite or
modify its checkout. Reports distinguish compilation failures, runtime failures,
wrong output and reference failures. Add `--strict` to return a failing status
when a comparison fails. See [tests/ofort/README.md](tests/ofort/README.md) for
selection files, discovery exclusions and reproducible checkpoints.

## License

The combined compiler is licensed under **[GPLv3](LICENSE)**. Original Rust
modules added in this fork and listed in [LICENSING.md](LICENSING.md) are also
available individually under the **[MIT license](LICENSE-MIT)**. Their source
files carry MIT SPDX headers. Upstream code remains subject to GPLv3.

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
- Counted DO, unbounded DO, and DO WHILE loops, conditionals, SELECT CASE, and array operations
  including sections, explicit bounds, vector subscripts, WHERE and reductions.
- Functions and subroutines, PURE/ELEMENTAL checks, INTENT, OPTIONAL/PRESENT,
  named arguments, and supported generic procedure calls.
- Scalar and rank-1 through rank-15 pointer descriptors over fixed `TARGET`
  storage, including array views returned through subroutine output pointers.
- Modules compiled together or separately using persistent `.rmod` interfaces, scalar derived types with
  scalar, array, ALLOCATABLE and POINTER components, arrays of derived types with
  scalar numeric/LOGICAL components, type-bound
  procedures, and defined operators, including derived-type comparisons.
- List-directed output, a subset of formatted input/output, runtime format
  strings, nonadvancing output, read-only named files, and scratch-file input/output.
- Initialization checks: definite reads before assignment are compile errors;
  uncertain reads are checked at runtime, with source locations and an element
  number for array errors.

These are bounded implementations, not claims of full language support. Many
array operations require statically known shapes, and procedure calls use
specialization rather than a general Fortran descriptor ABI. Important gaps
include general runtime-dependent declaration bounds and unbounded CHARACTER storage, general
allocatable/pointer and polymorphic behavior, general CHARACTER-valued functions,
and broad file I/O support. The feature
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
Unused-declaration warnings are opt-in and run before lowering. Enable them with
`-Wall` (also `--Wall`/`--wall`) or `-Wunused`, or select categories individually:
`-Wunused-type`, `-Wunused-variable`, `-Wunused-but-set-variable`,
`-Wunused-parameter` (named constants), `-Wunused-dummy-argument`, and
`-Wunused-function` (functions and subroutines). `-Wunused-constant` and
`-Wunused-procedure` are aliases. The variable category also enables warnings
for assigned-but-never-read variables. Prefix an option with `-Wno-` to disable
it, for example `-Wall -Wno-unused-type`; matching options apply in command-line
order. `-Wunused-private` selects private module entities, and
`-Wno-unused-private` suppresses those warnings. `-Werror` promotes enabled
warnings to errors; `-Werror=unused-type` enables and promotes one category.
`-Werror` alone does not enable unused warnings.

Public module exports and public external procedures in library-only sources
are exempt. Private module entities are checked through references from public
exports and live source statements, including generic/operator interfaces,
bindings, and procedure-address references. Named constants used in kinds,
lengths, bounds, initializers or literal kind suffixes count as referenced.
Assigned dummy arguments and function results are useful outputs, and loop
control variables count as used. Diagnostics name the category and point to the
original declaration, including original files in a multi-source build.
`!#allow(unused-type)` suppresses a category in a source file;
`!#deny(unused-type)` enables it as an error. The older post-lowering unused
warnings have been replaced, so unused dummy arguments no longer produce
messages saying "parameter" or duplicate local-variable warnings.
`xunused.f90` demonstrates these diagnostics; run `rfort -Wall xunused.f90` or
`python tests/test_unused_warnings.py` for the regression checks.

`-Woptional-argument` warns when an OPTIONAL dummy is referenced without a
proven `PRESENT` guard. For example, `if (present(s2))` does not prove that `s1`
is present; the warning points to the source line which uses `s1`. The check
runs on procedure definitions before presence specialization, even when all
current calls satisfy a deliberately documented assumption such as `vec`'s
consecutive arguments. Positive/negated guards, nested branches, early returns
and whole-argument forwarding to known OPTIONAL dummies are recognized.
Logical `.and.`/`.or.` expressions are checked without assuming short-circuit
evaluation. The analysis is conservative and does not infer arbitrary caller
contracts or logical-variable aliases for `PRESENT`.
The warning is off by default and enabled by `-Wall`; disable it with
`-Wno-optional-argument` or promote it with `-Werror=optional-argument`.
`-Wno-unused` controls only unused-declaration warnings. Source directives
`!#allow(optional-argument)` and `!#deny(optional-argument)` also work.
Run `python tests/test_optional_warnings.py` for regression checks.

The default optimization level is `-O2`. `--verbose` shows compilation progress
and tool commands; `--quiet` suppresses that progress. `--run` executes the
linked program and propagates its failure status.

`rfort run main.f90` interprets a program without building an executable. Run mode
is a separate implementation that is much less tested than native compilation:
few regression tests exercise it, it can be orders of magnitude slower on loops,
and it can produce results that differ from the compiled program. Prefer the
native build (`rfort --run`) when correctness matters.

List source files with the main program last to compile them as one source group.
Alternatively, compile modules in dependency order with `-c`, then compile their
consumers and link the objects. Successful native compilation writes one
`<module-name>.rmod` interface for each module defined in the source group.
Existing `.obj`/`.o` files can also be supplied for linking. Free-form `.f90`,
`.f95`, `.f03`, and `.f08` inputs are accepted. The existing language limitations
still apply.

For example, on Windows:

```cmd
rfort -c kind.f90
rfort -c util.f90
rfort -c xutil.f90
rfort kind.obj util.obj xutil.obj -o xutil.exe
```

This workflow does not remove any remaining language blockers in `util.f90`.
On Linux/macOS the corresponding object extension is `.o`.
Interfaces default to the current directory. `-J <directory>` selects their output
directory and adds it to module searches; repeat `-I <directory>` to add search
paths. Searches check the output directory, current directory, explicit `-I`
directories in order, and the input source directory. For example:

```cmd
if not exist build mkdir build
rfort -J build\modules -c kind.f90 -o build\kind.obj
rfort -I build\modules -c util.f90 -o build\util.obj
```

The compiler loads transitive module dependencies without needing the original
source files. A module supplied in the current source group takes precedence
over a saved interface. Missing, incompatible, circular and stale dependencies
are diagnosed; rebuild a dependent module after changing one of its dependencies.
Interfaces are published only after successful code generation; failed compilation
does not replace an existing interface. Native linking requires the needed object
files to be supplied explicitly.

`.rmod` is rfort's versioned, target-specific format, separate from other compilers'
`.mod` files. It stores parsed module definitions and source information, including
private entities and procedure bodies needed by the current specialization passes.
Imported module procedures are specialized or generated privately in each consuming
object. Module variables, allocation descriptors, initialization masks and supported
initialized procedure locals have one definition in their owning module object and
are shared by its separately compiled users. This is not a public descriptor ABI
or binary compatibility with gfortran. Recompile consumers when module definitions
change. `check` and interpreted `run` also load interfaces but do not publish them.
Run `python tests/test_module_compilation.py` for the separate-compilation regressions.

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

## Declaration diagnostics

Unsupported ELEMENTAL declarations and errors while resolving types or
constant initializers now report the source filename, line, and column and
underline the relevant attribute, name, or statement. For example, an unsupported
POINTER ELEMENTAL function dummy is diagnosed at its declaration, naming the argument
and procedure. The same reporting is used by native builds, `check`, and `run`.

Native multi-source builds retain explicit source ranges so these diagnostics
refer to the original file and line instead of `main.f90c-sources.f90`.
Declaration source offsets remain available through AST transformations;
passes that have not yet attached source locations retain their existing text
errors. Run `python tests/test_source_diagnostics.py` to check CLI modes,
continuations, repeated names, type errors, and original-file mapping.

## CHARACTER named-constant substrings

Scalar CHARACTER PARAMETER values support substring ranges such as
`upper_case(n:n)`. Constant bounds fold to CHARACTER literals, including in
PARAMETER initializers and LEN inquiries. Runtime bounds use the same checks
as scalar variable substrings. Omitted bounds, padded declared lengths and
empty ranges are supported; scalar substring strides and writes to PARAMETER
values are rejected. CHARACTER array PARAMETER sections remain array sections.

`INDEX`, `SCAN` and `VERIFY` support scalar CHARACTER arguments, optional BACK
and constant KIND (1, 2, 4 or 8), positional and named arguments, PARAMETER
initializers and PURE procedures. They search the full string, including trailing
blanks. INDEX returns one for an empty substring, or LEN(STRING)+1 with BACK true.
Rank-one CHARACTER array arguments are expanded elementally with shape checks;
BACK currently must be scalar. LEN_TRIM and character-code conversions also work
on fixed rank-one CHARACTER arrays. Run `python tests/test_character_intrinsics.py`
and `python tests/test_parameter_substrings.py` for interpreter/native O0/O2
checks and gfortran comparisons. `tests/157_parameter_substrings.f90` reproduces
the `upper_case_str` blocker in `util.f90`.

ADJUSTL/ADJUSTR work in assignments, concatenations, comparisons and fixed
rank-one array expressions; results keep the input length. Old-style
`character*5` and `character*(expression)` declarations are accepted. NEW_LINE
supports scalar CHARACTER arguments. Formatted PRINT may omit its output list,
and the colon format descriptor ends processing when the output list is exhausted.
Intrinsic operators may be extended for unsupported operand combinations such as
CHARACTER + CHARACTER; intrinsic arithmetic cannot be replaced. Run
`python tests/test_character_operators.py` for the CHARACTER operator subset.

## CHARACTER array parameters

Local rank-one CHARACTER PARAMETER arrays support explicit extents and `(*)`
extents inferred from their constant constructor. For example,
`character(len=2), parameter :: s(*) = ["aa", "bb", "cc"]` declares three
constant elements. Each element is padded or truncated to the declared length;
`len=*` can infer a common element length from a nonempty constructor.
Constant element references, copies of named array parameters, constructor
implied-DOs, and fixed nonunit lower bounds are supported. Writes and initializer
shape mismatches are rejected. Storage retains the existing local rank-one
CHARACTER array limit of 1024 elements; this does not add general assumed-size
dummy arrays. Run `python tests/test_character_array_parameters.py` for
interpreter/native O0/O2 checks and gfortran comparisons.

## Scalar CHARACTER functions

Ordinary scalar CHARACTER functions defined in the source group now specialize
calls into caller-owned result buffers. Fixed-length and call-site constant
result lengths, including `len(a)` and `len_trim(a)` for constant actuals,
support assignments, concatenation, formatted output, named and nested calls,
early RETURN, OPTIONAL inputs, and procedures with no arguments. Scalar dummies
require explicit INTENT. Writable arguments must be whole variables, and locals
must be explicitly declared.
Constant length evaluation respects fixed dummy lengths, including truncation
of a longer actual argument.
CHARACTER result and local lengths can use `SIZE(array)` and `SIZE(array,dim)`
with actual array shapes known during specialization, including sections and
multidimensional arrays. For example, `join_fixed` can declare its result as
`character(len=size(words)*nlen)` when `nlen` is a constant actual argument.
`tests/182_character_result_size.f90` exercises this case with runtime formatted
internal output.

Deferred-length ALLOCATABLE scalar results use the existing bounded CHARACTER
storage lowering for calls from the main program. Assignment sets the runtime
result length, so concatenation, LEN and assignment to a caller's allocatable
variable preserve that length. Runtime lengths for explicitly sized results,
recursive CHARACTER calls, host captures, and a
separate-object CHARACTER result ABI remain unsupported. Runtime-length results
as actual arguments to other procedures remain outside this subset.

`tests/146_character_functions.f90` reproduces `xchar_func.f90`, including its
allocatable-result function. Run `python tests/test_character_functions.py` for
interpreter/native O0/O2 checks and gfortran comparisons. Unused CHARACTER
function definitions are removed before native result-storage validation;
this allows the `exe_name` definition in `util.f90` to pass that blocker.
GETCWD and GET_COMMAND_ARGUMENT themselves remain unsupported when called.

## Forwarded array-valued function calls

Supported array-valued functions can be called from procedures with assumed-shape
arguments. Function expansion waits for procedure association to supply the
actual extents and CHARACTER lengths. This applies to named and defined-operator
calls, including `xfind .in. xdata`, and to numeric, LOGICAL and CHARACTER results.
Nested forwarding, keyword calls, empty arrays and supported constant CHARACTER
sections are covered by `python tests/test_forward_array_results.py`.

Bounded function arguments and fixed-shape results support ranks 1 through 15,
independently of PURE; argument and result ranks may differ. This includes
numeric, LOGICAL and fixed-length CHARACTER arrays. Each dimension is preserved
until storage lowering, with Fortran column-major element order. Read-only
inputs are copied; explicit INTENT(OUT)/INTENT(INOUT) variables retain caller
storage and side effects. These specializations still require at most 1024
elements per array, with extents and CHARACTER lengths known after association.
Source array elements copied into the function must be
initialized. Rank-one fixed-length CHARACTER ALLOCATABLE results can use bounded
ALLOCATABLE output helpers or direct `ALLOCATE` statements with proven finite
bounds; their active length is tracked separately from storage capacity. This
supports filtered `.exclude.` results and OPTIONAL `vec` calls with zero through
ten consecutive arguments, including empty results, assignment, argument passing
and list-directed/formatted output. `tests/183_optional_character_array_result.f90`
tests the original `util.f90` routine and preserves its consecutive-argument
assumption.
`python tests/test_character_allocatable_results.py` checks these paths against
the interpreter, native code at `-O0`/`-O2`, and gfortran when available.
`python tests/test_multirank_array_functions.py` covers PURE and ordinary
functions, ranks through 15, differing result ranks, sections, empty results,
array expressions, writable arguments and module side effects. `xabove_diag.f90`
prints `4.0 7.0 8.0` with or without PURE.

## PURE functions inside FORALL

FORALL assignments can call supported PURE scalar and array-valued functions,
including numeric array functions and CHARACTER elemental functions. Constructors
can combine scalar items and array-valued results, as in
`[0.0_real64, cumulative_sum(x(:,j))]`. Function arguments can use fixed matrix
sections and empty arrays. Impure calls remain errors.

Every right-hand side is captured before any destination write, preserving
overlapping assignments such as swapping matrix columns. Captures run after
procedure argument specialization when assumed-shape dummy extents were unknown
in the original template. Runtime scalar INTEGER bounds and strides, including
negative strides, are supported for fixed destinations and descriptor-backed
numeric or LOGICAL destinations. Controls are evaluated
once, and duplicate destination elements cause a runtime error. The enclosing
index variables keep their original values. Constant triplets retain their
existing compile-time expansion and destination checks. Existing limits remain:
one assignment per FORALL and bounded array-result specialization. Runtime-sized
destinations require descriptors; runtime-sized CHARACTER destinations remain
unsupported. Calls in WHERE remain
outside this subset. Numeric, LOGICAL and fixed rank-one CHARACTER destinations
are supported. Run `python tests/test_forall_functions.py` and
`python tests/test_runtime_forall.py`.

CHARACTER array arguments retain their active lengths when their backing storage
has greater capacity. Function and subroutine specialization revisits nested
calls exposed by descriptor association, including scalar search functions inside
array-valued search functions. This supports the `unique_characters` utility
driver's unique values, first positions, frequencies and group numbers. Run
`python tests/test_character_active_extent.py` for nested calls, differing
CHARACTER lengths, empty arrays and repeated allocation regressions.

## Defined operators with array operands

Defined dotted operators such as `.notin.` can select functions with array
operands. Functions with read-only INTEGER, REAL, DOUBLE PRECISION
or LOGICAL array dummies specialize for statically known actual extents, including
assumed-shape dummies, fixed sections, constructors, supported array expressions
and empty arrays. The same specialization works for direct function calls and
supported numeric/LOGICAL results of ranks 1 through 15 whose extents resolve at
the call site. Input type, rank, extent and initialization checks remain active.

Scalar numeric and LOGICAL results can use runtime-sized array operands through
the descriptor association described below. Array-result specialization still
copies bounded inputs and supports at most 1024 elements per dummy. Recursive
specialization and unrestricted array-result ABIs remain unsupported. `xnotin.f90` now prints `F T`. Run
`python tests/test_array_operators.py` for interpreter/native O0/O2 checks and
gfortran comparisons.

## CHARACTER array functions

CHARACTER functions can accept read-only arrays and return fixed-shape arrays
of ranks 1 through 15. Calls specialize assumed lengths and shapes from actual
arguments; result lengths and extents may use LEN and SIZE of the dummies.
Inputs and results use local buffers with at most 1024 elements per array.
Literal constructors, typed empty constructors, fixed arrays, constant sections,
named calls, and another CHARACTER array function's result are supported as
actuals. The input arrays need not have equal extents. Fixed-length array dummies
currently require the actual element length to match; explicit-shape dummies
require matching extents. Scalar INTEGER constant actuals can supply result
extents. Runtime-dependent result extents, nonunit declaration bounds in these calls,
POINTER/ALLOCATABLE array dummies and deferred-length array results remain
unsupported. Read-only array actuals are copied and must be initialized.

CHARACTER FORALL assignments to fixed rank-one arrays with lower bound one
now use the existing
numeric FORALL snapshot rules: capture all right-hand sides before destination
writes, check bounds, and reject duplicate destinations. Triplets and destination
subscripts must resolve to constants; stable local constants can be used in
subscripts after function specialization. Supported PURE CHARACTER procedure
calls inside FORALL use the same snapshots. Function results support assignment, concatenation,
formatted output, LEN/SIZE inquiries, nested calls, and early RETURN.

`tests/148_character_array_functions.f90` reproduces `xmunge.f90` and the
`munge` function in `util.f90`. Run `python tests/test_character_array_functions.py`
for interpreter/native O0/O2 checks and gfortran comparisons, including empty
arrays, zero element lengths, input sections, and overlapping FORALL assignment.

Functions returning numeric or LOGICAL rank-one arrays can use the same
specialization when they have CHARACTER input dummies. Assumed CHARACTER
lengths bind to each actual argument, including strings read at runtime;
the result extent must still be known at the call site. Read-only rank-one
array inputs, named and nested calls, empty arrays, and early RETURN are
supported with the limits above. This does not add a separate-object CHARACTER
argument ABI or multidimensional array results.
`tests/149_numeric_array_character_arg.f90` reproduces the `position` argument
blocker from `util.f90:combine_vec_mat_real` with a rank-one result.
Run `python tests/test_numeric_array_character_arg.py` for interpreter/native
O0/O2 checks and gfortran comparisons.

Functions with read-only rank-one CHARACTER array inputs can also return
numeric or LOGICAL scalars. This includes assumed element lengths, typed
function headers, named calls, fixed array sections, empty arrays, and CHARACTER
array function results as actuals. Array extents must be known at the call site;
their string contents can be read at runtime. The same input initialization,
rank, and size limits above apply. `tests/150_character_array_scalar_result.f90`
reproduces the CHARACTER dummy blocker in `util.f90:num_unique_char`.
Run `python tests/test_character_array_scalar_result.py` for interpreter/native
O0/O2 checks and gfortran comparisons.

## Elemental function results

ELEMENTAL functions support explicitly declared numeric, LOGICAL, and CHARACTER
scalar results and input dummies. CHARACTER results and CHARACTER input dummies
are specialized into local buffers for each call. Assumed lengths follow the
actual argument, and result lengths such as `len(a)+len(b)` resolve using those
lengths. Fixed-length input dummies may consume the leading part of a longer
actual. Assignments retain normal padding and truncation.

Scalar calls, rank-one array constructors/variables/constant sections, scalar
broadcasting, nested calls, named arguments, early RETURN, empty arrays, and
formatted PRINT/WRITE are supported for CHARACTER elemental functions.
Scalar actual expressions are evaluated once per array call; IMPURE element
calls run in array element order. LOGICAL results also work with the existing
numeric array expansion. Purity, INTENT(IN), shape compatibility, argument
kinds, and uninitialized reads remain checked.

CHARACTER lengths must be constant after specialization, and locals must be
explicitly declared. Runtime-length actuals
such as TRIM of a variable, higher-rank arrays in CHARACTER calls,
recursive CHARACTER calls, enclosing-procedure variable captures, internal
procedures inside these functions, and CHARACTER calls in WHERE remain
unsupported and are diagnosed. This uses the compiler's existing fixed-size
local CHARACTER storage, not a general CHARACTER function-result ABI.
Run `python tests/test_character_elemental.py` for interpreter/native O0/O2
checks with gfortran comparisons. The main suite includes
`tests/143_character_elemental.f90`, matching `xchar_plus_char.f90`.

## Elemental subroutine setters

Scalar calls to ELEMENTAL subroutines support explicit INTENT(IN), INTENT(OUT),
and INTENT(INOUT), omitted OPTIONAL arguments, named arguments, and PRESENT tests.
Intrinsic purity classifications are shared through `src/intrinsics.rs`. PURE
procedures recognize rounding intrinsics, SIGN/DIM/DPROD, MERGE, array products,
lexical CHARACTER comparisons and ASSOCIATED, plus the existing intrinsic
families. RANDOM_NUMBER, RANDOM_SEED, CPU_TIME, SYSTEM_CLOCK and DATE_AND_TIME
remain impure. User-defined procedure declarations take precedence over intrinsic
names. Classification tests cover all registered intrinsic interfaces, while
`python tests/test_pure_intrinsics.py` checks supported execution and rejection
of impure calls. Some classified intrinsics still have incomplete semantic or
execution support; the purity catalog records their language classification.

`NINT(A[,KIND])` rounds REAL and DOUBLE PRECISION values to the nearest integer,
with halfway cases rounded away from zero. Scalar and supported array arguments,
keyword arguments, and INTEGER result kinds 1/2/4/8 are supported. Inlined
functions and subroutines also preserve early RETURN from explicit-step DO loops,
including nested loops and CYCLE paths. Run `python tests/test_util_correctness.py`
for interpreter/native O0/O2 comparisons with gfortran.

ERROR STOP is permitted in PURE and implicitly pure ELEMENTAL procedures,
as in Fortran 2018. Its code expression is checked for impure calls, and
unconditional error termination ends definite-assignment analysis for that path.
Plain STOP remains forbidden in pure procedures. The existing stop-code support
accepts CHARACTER and default INTEGER constants. `xpure_error_stop.f90`,
`tests/173_pure_error_stop.f90`, and `python tests/test_pure_error_stop.py`
cover successful guards, error termination and integer exit statuses.

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
as described below. Scalar and elemental CHARACTER functions use the
specialization described above. Runtime-dependent explicit lengths and
CHARACTER functions outside those subsets remain unsupported and produce
diagnostics. Bounded deferred-length scalar assignment is described below.

## Formatted output

WRITE and PRINT accept scalar CHARACTER format expressions: literals, named
PARAMETERs, variables (including formats read from input), concatenation, and
TRIM. WRITE supports standard output (`*`, constant unit 6, or an INTEGER
unit variable containing 6), with or without
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
`ADVANCE='yes'` for the default behavior. ADVANCE also accepts scalar CHARACTER
variables and supported expressions, including specialized function results.
Its value is evaluated once and checked for case-insensitive `yes` or `no`,
allowing trailing blanks. Nonadvancing
writes flush stdout, so prompts appear before a following READ. A final
nonadvancing write leaves stdout without an added newline, including at STOP.
ADVANCE cannot be used with list-directed (`*`) formats.

Formatted internal WRITE to a scalar CHARACTER buffer supports literal or runtime
format strings, INTEGER/REAL/LOGICAL/CHARACTER items, arrays and implied-DO lists.
The same parser and descriptors are used for standard output. Each statement
captures its inputs before replacing the destination, pads unused characters
with blanks, and reports record overflow. PURE and ELEMENTAL procedures may
write local CHARACTER variables and writable dummy arguments; INTENT(IN),
nonlocal destinations and external I/O remain protected. This subset supports
one record and explicit formats; internal file arrays, list-directed output,
IOSTAT and other WRITE control options remain unsupported.
`xstr_real.f90`, `tests/172_internal_write.f90`, and
`python tests/test_internal_write.py` cover this behavior.
Captured array items also retain every element after vector selection and
function specialization. `xnow_format.f90` reproduces a calendar string written
from an integer vector selection; `python tests/test_util_regressions.py` checks
this alongside multidimensional, empty and runtime-sized array items.

Formatted PRINT/WRITE to standard output support nested implied-DO lists with
runtime INTEGER bounds and positive/negative steps. Controls are evaluated once
on entry to each implied-DO. Transfer values, including CHARACTER substrings,
are copied into a statement-local list before formatting. Memory use scales
with the list's item count and CHARACTER lengths.
Finite nested format groups and a final unlimited-repeat group such as
`(*(1x,a))` are supported. Unlimited groups require a data descriptor and repeat
until the output list ends; finite formats retain the existing exact item-count
requirement. Runtime formats can also consume numeric and LOGICAL array items
and sections using their active element counts. Empty CHARACTER array items
inside zero-iteration implied-DOs are not read. Run
`python tests/test_output_units.py` and `python tests/test_formatted_implied_do.py` for interpreter/native
O0/O2 checks with gfortran comparisons. The Cargo suite includes
`tests/139_formatted_implied_do.f90`.

Transfer items inside implied-DOs must resolve to supported scalar values;
existing CHARACTER procedure-argument limitations remain. Numeric format labels,
format reversion remain unsupported outside the existing `(A)` file subset.
Formatted array items and implied-DO lists can also write sequential file units.
Format nesting is limited to 32 levels;
the existing limits of
256 for field width/precision and 10,000 for repeat counts apply.
The native formatting runtime is written in Rust, built with the compiler,
and embedded in `rfort`; no separate runtime installation is needed. Use
`rfort link` when linking emitted objects so it supplies this runtime.

## File input

Sequential formatted files support `OPEN(unit=20,file='data.txt',status='old',action='read')`,
`READ(20,'(a)') text`, `BACKSPACE(20)`, `REWIND(20)`, and `CLOSE(20)`.
FILE may be a runtime scalar CHARACTER expression. Named files also support
NEWUNIT, STATUS='replace' with ACTION='write' or 'readwrite', and
STATUS='old' with ACTION='read', 'write' or 'readwrite'. The default
STATUS='unknown', ACTION='readwrite' combination opens or creates a file.
POSITION='rewind' is supported. Explicit units are
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

List-directed READ uses a shared streaming scanner in native and interpreted
execution. There is no fixed record-length limit: storage grows with token
length, rather than record length. A statement can read multiple INTEGER
(kind 1, 2, 4 or 8), REAL (kind 4 or 8), LOGICAL, CHARACTER and COMPLEX
(kind 4 or 8) values, continuing across records as necessary. INTEGER range
checks, E/D exponents, omitted exponent letters, repetition, null fields,
slash termination, quoted strings, doubled quotes and quoted continuation
across external records are supported. Null/slash fields preserve the old
value and its initialization state. The remainder of the final record is
consumed before the next advancing READ.

Supported destinations include numeric/LOGICAL arrays, runtime-sized arrays,
multidimensional triplet sections, bounded CHARACTER arrays and scalar
COMPLEX or whole COMPLEX arrays. Nested input implied-DO lists accept runtime
bounds and positive or negative steps. IOSTAT accepts INTEGER kinds 1, 2, 4
and 8. Standard input (`*` or unit 5), connected external units and scalar
CHARACTER internal files use the same scanner. Run
`python tests/test_list_read.py` for interpreter/native O0/O2 checks against
gfortran, including `tests/io/write_read.f90`, a copy of `xwrite_read.f90`.

This is not yet complete Fortran input support: CHARACTER internal-file arrays,
COMPLEX section destinations, vector subscripts, derived-type input and defined I/O,
DECIMAL='COMMA', IOMSG and combined status controls remain unsupported.

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
`END=label` on a list-directed or scalar CHARACTER formatted READ branches
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
on close. `INQUIRE` accepts exactly one of `UNIT=` or `FILE=`, with any
combination of scalar-variable outputs `OPENED=`, `NUMBER=`, `NAME=` and
`IOSTAT=`. Controls may be reordered; UNIT may also be positional. Unit queries
recognize preconnected units 5 and 6 and scratch NEWUNIT connections. Unconnected
units and filenames return OPENED false, NUMBER -1 and IOSTAT zero. Invalid
queries return a nonzero IOSTAT, or stop with an error when IOSTAT is omitted.
Named connections return their OPEN path in NAME, padded or truncated to the
destination's declared length; scratch and unconnected queries fill NAME with
blanks (NAME is undefined by Fortran for an unnamed/unconnected unit).
Filename lookup ignores trailing blanks and compares normalized absolute paths;
distinct hard-link names are not equated. Run `python tests/test_inquire.py` for
interpreter/native O0/O2 checks against gfortran, including connection changes.
`(A)` READ destinations may also be dynamically indexed local CHARACTER elements.

External list-directed WRITE, file ADVANCE controls, and IOSTAT on OPEN/CLOSE
or positioning operations remain unsupported.
Connections are shared within a
compiled source group. Native file I/O currently requires a 64-bit target.
Run `python tests/test_read_comments.py` for interpreter/native O0/O2 and
gfortran comparisons of these features.

Internal list-directed READ uses the shared scanner described above, including
mixed-type input lists and bounded CHARACTER-array destinations. Scalar
CHARACTER destinations use blank padding or truncation. Failed scalar transfers
leave their destination undefined; null fields preserve its definition status.

## Numeric and logical reductions

The GNU `ISNAN(X)` extension is supported for REAL kinds 4 and 8. It is pure
and elemental, returning default LOGICAL with the input's shape. NaNs return
true; finite values, signed zero and infinities return false. Scalar expressions
are evaluated once. Arrays, sections, empty arrays and runtime PACK results are
supported, as is the `X=` keyword. Run `python tests/test_isnan.py` for
interpreter/native O0/O2 comparisons with gfortran in GNU mode, including ranks
through 15 and runtime-sized multidimensional arrays.

`GET_COMMAND_ARGUMENT(NUMBER, VALUE, LENGTH, STATUS)` supports keyword
arguments and optional outputs. NUMBER, LENGTH and STATUS use default INTEGER.
VALUE is blank-padded or truncated; LENGTH reports the complete argument length.
STATUS is 0 for success, -1 for truncation, and positive for a missing argument.
Argument zero is the native executable name, or the source filename when
interpreting. Scalar array-element outputs and assumed-length CHARACTER dummies
are supported, including zero-length VALUE buffers. The intrinsic is impure.
Pass program arguments after `--`: `rfort main.f90 --run -- "hello world"` or
`rfort run main.f90 -- "hello world"`. Arguments after `--` retain spaces, empty
strings and leading hyphens. Run `python tests/test_command_argument.py` for
interpreter/native O0/O2 and gfortran comparisons.

The GNU `GETCWD` subroutine is supported as `CALL GETCWD(C, STATUS)` with
optional STATUS, including keyword arguments. C is a writable scalar CHARACTER
variable or array element; STATUS is INTEGER kind 4 or 8. Success returns 0 and
blank-pads C. On failure C is blank and STATUS receives a nonzero host error
code. The function form is not implemented. Run `python tests/test_getcwd.py`
for interpreter/native O0/O2 and gfortran comparisons. Initialization errors
now include the source statement location, including calls in combined files.

`GET_ENVIRONMENT_VARIABLE(NAME, VALUE, LENGTH, STATUS, TRIM_NAME)` supports
positional and keyword arguments. NAME accepts scalar CHARACTER text and
TRIM_NAME defaults to true. VALUE is blank-padded or truncated; LENGTH reports
the full value length. STATUS is 0 for success, -1 for truncation, and 1 for a
missing variable. LENGTH and STATUS require default INTEGER outputs. The
intrinsic is impure. It reads the host C runtime environment on Windows, Linux
and macOS; Windows treats an empty environment assignment as removal. Run
`python tests/test_environment.py` for interpreter/native O0/O2 and gfortran checks.

FLOOR and CEILING accept scalar or elemental REAL inputs, including array
sections and runtime-sized arrays. Both support `A=` and optional constant
`KIND=` (INTEGER kinds 1, 2, 4 and 8), and constant PARAMETER initializers.
Unrepresentable results, including NaN and infinity, report an error instead
of converting silently. Run `python tests/test_rounding.py` for interpreter,
native O0/O2 and gfortran comparisons.

Numeric `MIN` and `MAX` accept two or more same-type, same-kind arguments in
array expressions. Scalar arguments broadcast across conformable arrays;
sections and runtime-length PACK results are supported, including empty arrays.
This also permits scalar extrema of reductions inside array expressions, such
as `x / max(sum(abs(x)), tiny_value)`. Shape mismatches are checked before
runtime-length element loops. Run `python tests/test_numeric_minmax.py` for
interpreter/native O0/O2 and gfortran comparisons. Errors from numeric array
lowering now include the source statement's location.

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

PURE functions with scalar INTEGER, REAL, DOUBLE PRECISION or LOGICAL results
can take read-only numeric/LOGICAL array arguments with fixed or assumed shapes.
Calls reuse specialized subroutine argument association, so forwarding through
another procedure and runtime-sized arrays use the actual array descriptors.
Numeric assumed-shape inputs support ranks 1 through 15, with extents checked
at runtime as needed. Assumed-shape dummy lower bounds start at 1. Scalar-result
functions with read-only CHARACTER inputs also use subroutine association;
existing CHARACTER storage and extent limits apply.

Whole arrays, empty arrays, constructors, supported sections and array
expressions are accepted. Runtime array expressions are evaluated into temporary
heap storage before association. Array elements are checked for initialization
when read; a SIZE inquiry does not require initialized data. Explicit-shape
numeric function inputs retain the existing fixed-size ABI and its 1024-element
limit. OPTIONAL, POINTER and ALLOCATABLE dummies and array-valued results retain
their existing specialization limits. Recursive functions with array dummies remain unsupported.
Run `python tests/test_scalar_array_functions.py` and
`python tests/test_function_arrays.py`.

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
PRECISION and RANGE also accept COMPLEX kinds 4 and 8. General procedure-call
arguments for these inquiries are not implemented.
Variables and user procedures may still use these intrinsic names.

`SCALE`, `FRACTION`, `EXPONENT`, `SET_EXPONENT`, `NEAREST`, `SPACING`, and
`RRSPACING` support REAL kinds 4 and 8, keyword arguments, scalar expansion,
conformable arrays, PURE procedures, and supported constant expressions.
EXPONENT returns default INTEGER; the other results retain X's real kind.
SCALE and SET_EXPONENT accept INTEGER kinds 1/2/4/8 for I. NEAREST's nonzero
REAL direction argument may have a different kind from X; a zero direction
is diagnosed at compile time when constant and otherwise at runtime.
The shared binary implementation handles signed zero, subnormal values, and
scaling across the representable range without an intermediate power overflow.
Run `python tests/test_float_model.py` for interpreter/native O0/O2 and gfortran
comparison checks.

List-directed LOGICAL output uses `T` and `F` for scalar values, array elements,
expressions and procedure results. CHARACTER strings containing `.TRUE.` or
`.FALSE.` retain their spelling. Run `python tests/test_logical_list_output.py`
for strict spelling checks in the interpreter and native code at O0/O2.
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
pointer ALLOCATE remain future work. Numeric ALLOCATABLE heap storage is
described below. Early IR transformations that assume
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
overlapping assignment semantics.

Constructor implied-DO expansion can wait until procedure specialization resolves
bounds such as `size(xx)` for an assumed-shape dummy. Runtime-bound constructors
with scalar items also support whole-array assignment to bounded rank-one
numeric/LOGICAL storage. Controls are captured on entry to each implied-DO;
constant positive/negative strides, dependent nested bounds, mixed scalar items,
and empty iteration ranges are supported. Each index uses local storage and
retains its declared INTEGER kind. The enclosing index variable is unchanged.
A temporary captures every result before destination writes; too many elements
or a final shape mismatch produce a runtime error.

Runtime-bound constructors can also be ordinary expression values, including
arithmetic, reductions, procedure arguments, and control expressions. Their
storage grows at runtime, so these uses do not need a compile-time capacity.
Nested loops, runtime strides, zero-trip ranges, and whole-array items in array
element order are supported. Bounds and items are evaluated once at their
respective evaluation sites; constructor indices retain local scope. Fixed-length
CHARACTER scalar items also support runtime construction and assignment.
The bounded numeric assignment path remains an optimization for existing cases.
Some array-valued expressions and sections inside runtime constructors, and
CHARACTER function results requiring later specialization, remain unsupported.
PARAMETER and declaration initializers still require constant expressions. CHARACTER scalar-element constructors can be passed to
read-only CHARACTER array dummies, including typed empty constructors. Constant
formats can print fixed whole numeric/LOGICAL arrays in element order; runtime
format strings with whole numeric/LOGICAL array lists remain unsupported.
Run `python tests/test_constructor_expressions.py` and
`python tests/test_runtime_constructors.py` for interpreter/native O0/O2
checks and gfortran comparisons; `tests/147_runtime_constructors.f90` reproduces
`xconstructor_implied_do.f90` and the comparison expression in `util.f90`.

Empty typed bracket constructors preserve the declared element type and kind:
`[real(kind=dp)::]`, `[integer(kind=8)::]`, `[logical::]`,
`[character(len=3)::]`, and `[double precision::]` all have size zero.
Named kind and length parameters, including imported parameters, are resolved
before array lowering. These constructors support type/size inquiries,
reductions, zero-length assignments, numeric and CHARACTER array PARAMETER initializers,
and procedure arguments under the compiler's existing array-shape rules.
Nonempty explicitly typed constructors currently require constant elements.
Untyped empty constructors remain invalid. Run
`python tests/test_empty_constructors.py` for interpreter/native O0/O2 checks
with gfortran comparisons; the Cargo suite includes
`tests/142_empty_constructors.f90`.

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
NON_INTRINSIC requires a user module in the source group or a saved `.rmod` interface. Other intrinsic
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
`(-2)**2` is 4, and `2**3**2` is 512. INTEGER PARAMETER declarations support
kinds 1, 2, 4 and 8, preserving kind and checking the declared range.
Kind-16 integer literals remain unsupported. Run
`python tests/test_integer_literals.py`.

Named constants support INTEGER, REAL (kinds 4 and 8), DOUBLE PRECISION,
default LOGICAL and CHARACTER scalars and rank-one arrays. LOGICAL initializers
support literals, previously defined constants, `.not.`, `.and.`, `.or.`,
`.eqv.`, `.neqv.`, scalar numeric/CHARACTER comparisons, and ALL/ANY of logical
constructors or named constant arrays (including empty arrays). CHARACTER
comparisons pad the shorter operand with spaces; INTEGER comparisons retain
integer precision. Module imports, host association and separate PARAMETER
statements use the same constant resolution. Constant arrays still have the
existing rank-one limits; COMPLEX constants support the subset described below.
Derived-type named constants are not implemented. Run `python tests/test_named_constants.py`.

## Declaration initialization

Main-program variables support declaration initialization from supported constant
expressions. Numeric scalars, rank-one array constructors (including constant
implied-DOs), scalar array fills, supported LOGICAL constant expressions and fixed-length CHARACTER
scalars are supported. Values remain mutable; initialization runs once before
normal use, including declarations with an explicit SAVE attribute. INTEGER
initializers currently require integer constant expressions. Additional constant
array transformations remain outside this subset. Run `python tests/test_main_initialization.py`.

Initialized INTEGER, REAL, DOUBLE PRECISION, LOGICAL and fixed-length CHARACTER
scalar locals in functions
and subroutines now have persistent SAVE storage, including locals declared inside
procedure BLOCK scopes. Declaration initialization implies SAVE even when the
attribute is omitted. Values survive repeated calls, OPTIONAL specializations and
recursive activations; each declaration has independent storage. Ordinary locals
without initialization retain their existing behavior. PURE procedures reject
saved initialized locals. Saved arrays, TARGET locals, module
initialization and SAVE declarations without initializers remain unsupported.
Run `python tests/test_saved_locals.py` and `python tests/test_saved_characters.py`;
`xsave_counter.f90` is a small numeric example.

## OPTIONAL procedures

Scalar OPTIONAL arguments retain their positions during derived-type lowering,
including calls to ordinary procedures, bound procedures and functions returning
a derived value. An unrelated derived type in the same source group no longer
makes omitted arguments required. `xstr_int.f90` and
`tests/test_derived_optional.py` cover omitted and reordered arguments, forwarding
absent dummies, and component expansion around optional arguments.

Derived-type lowering respects local array declarations and host association:
array subscripts are not associated with a same-named procedure's arguments.
Calls that do not use derived arguments or results pass through without argument
rewriting. `xivec_shadow.f90` and `tests/test_derived_shadowing.py` cover this
behavior, including comparisons with and without an unrelated derived type.

Scalar derived objects can have array, ALLOCATABLE array and POINTER (scalar or
array) components. Each component is stored as its own variable, so element and
section references, ALLOCATE/DEALLOCATE, ALLOCATED/ASSOCIATED, pointer
assignment, NULLIFY and passing a component to an array dummy behave as they do
for ordinary variables. Whole-object assignment deep-copies ALLOCATABLE
components (keeping their bounds, or leaving the target unallocated) and copies
POINTER association. Derived arguments, INTENT(OUT) dummies and derived function
results carry these components. A constructor value for an ALLOCATABLE component
is assigned with the reallocation rules below; `null()` or an omitted ALLOCATABLE
or POINTER component becomes unallocated or disassociated.
A component may itself have a derived type (`type(point) :: a`); `s%a%x`,
whole nested values such as `s%a = point(1, 2)`, nested constructors and passing
`s%a` to a `type(point)` dummy work as for top-level objects. Arrays of derived
type as components, ALLOCATABLE or POINTER components of derived type,
ALLOCATABLE scalar components (including deferred-length CHARACTER), arrays of
derived types with non-scalar components and functions with ALLOCATABLE array
dummies remain unsupported, and an ALLOCATABLE component, like any ALLOCATABLE
array, must be allocated (by ALLOCATE or by assignment) somewhere before it is
referenced. Run `python tests/test_derived_array_components.py`.

Dummy procedures can be declared EXTERNAL (including `real(8), external :: f`),
in an interface block, with `procedure(iface)` naming an ABSTRACT interface or
another procedure, or simply by being invoked. Each call passing procedure names
is redirected at compile time to a copy of the callee with the dummies replaced
by those procedures, so calls through dummies are direct calls. Module,
internal and external procedures can be passed, a dummy procedure can be
forwarded to another procedure, and keyword actuals work. Procedure pointers
declared with `procedure(iface), pointer` become an integer tag: `p => f`,
`nullify(p)`, `associated(p)` and `associated(p, f)` update or test it, and calls
through `p` dispatch on it to the procedures ever associated with `p` in its
scope. Procedure pointer components, procedure pointers passed as actual
arguments or used as dummies, OPTIONAL dummy procedures and passing intrinsic
procedures are not yet supported. A module containing a procedure with dummy
procedures must be compiled with its callers, since the copies are made per
call. Run `python tests/test_procedure_arguments.py`.

`call move_alloc(from, to)` moves an ALLOCATABLE array, including components,
by copying it and deallocating `from`, so pointers into `from` are not carried
over. ALLOCATE accepts `source=` and `mold=` (taking the bounds of an array
source or mold, or the given bounds), and ALLOCATE and DEALLOCATE accept
`stat=` and `errmsg=`, which report allocating an allocated array or
deallocating an unallocated one instead of stopping. Run
`python tests/test_allocation_options.py`.

ASSOCIATE replaces each associate name by its selector inside the block, so a
variable, element, section or component selector is updated through the name.
A selector whose value could change inside the block, such as `a(i)` when the
block assigns `i`, is rejected because it would be re-evaluated. ASSOCIATED
also accepts a TARGET (a TARGET variable, section or POINTER) and is true when
the pointer is associated with the same elements in the same order. Run
`python tests/test_associate.py`.

Intrinsic assignment of an array value to a whole heap-backed numeric or LOGICAL
ALLOCATABLE array allocates it when unallocated and reallocates it when the
shape differs, as in Fortran 2003. A reallocated target takes the lower bounds
of a whole array source and otherwise starts at 1; a target whose shape already
conforms keeps its bounds. The right-hand side is captured first, so it may
refer to the target. Rank-one targets accept any array expression. Targets of
rank two or more reallocate from whole arrays and sections; other
multidimensional expressions still require a conforming allocated target.
Scalar assignment never allocates. Array constructors may contain whole
runtime-sized arrays and sections, so `a = [a, x]` appends. CHARACTER arrays and
rank-one ALLOCATEd locals of procedures with array dummies (which use bounded
call-site storage) are not yet reallocated. Run
`python tests/test_allocatable_reallocation.py`.

Local OPTIONAL functions and subroutines can be specialized for positional and
named calls, including omitted arguments before later named arguments and
forwarding an absent optional dummy to another optional dummy. PRESENT
and simple logical combinations of presence tests select the applicable branches;
a missing required argument or an unguarded absent-dummy reference is diagnosed.
Optional functions currently require explicit INTENT(IN) on their arguments.
Recursive calls reuse the specialization for each argument-presence combination.
A separate-object optional-argument ABI remains unsupported. Named calls require
a definition in the source group.

ELEMENTAL functions also support OPTIONAL numeric, LOGICAL, and CHARACTER
inputs, scalar broadcasting, conforming array actuals, and absent-argument
forwarding. Numeric elemental array results can be passed to array dummies.
Specialized presence branches retain source-level parameter-use information
for unused-argument warnings. `tests/144_optional_elemental.f90` tests the
`sgn_real` example with scalar and array calls. INTEGER constant ERROR STOP
codes are supported in native programs and the interpreter, with the code
used as the process exit status. Run `python tests/test_optional_elemental.py`
for these checks and gfortran comparisons.

Scalar CHARACTER subroutine arguments support fixed and assumed lengths,
including OPTIONAL inputs and writable variables. Assumed lengths follow each
actual argument; LEN, empty strings and literal/concatenated inputs work. Fixed
lengths currently require an exact match. Expressions require INTENT(IN), while
writable arguments must be variables. Explicit RETURN leaves an expanded
subroutine, including from nested loops, and execution resumes in its caller.
Fixed-length rank-one CHARACTER array subroutine arguments also use call-site
specialization, including assumed-length dummies.
Run `python tests/test_optional_procedures.py`.

Ordinary functions returning numeric or LOGICAL scalars also specialize
read-only CHARACTER scalar inputs, binding fixed or assumed lengths at the
call site. Known definitions in the source group, named and nested calls,
OPTIONAL inputs, empty strings, runtime string contents and early RETURN
are supported with the existing CHARACTER function limits. This covers
`util.f90:assert_eq2`, reproduced by `tests/155_scalar_character_arguments.f90`.
STOP with a CHARACTER literal or named constant prints its message to stderr
and terminates normally with status zero; a default INTEGER literal or named
constant prints its code and uses it as the process exit status. Bare STOP
remains silent, and PURE procedures reject STOP. `tests/156_stop_code.f90`
checks termination with an INTEGER code. Run
`python tests/test_scalar_character_arguments.py` for interpreter/native O0/O2
checks and gfortran comparisons of function results.

Local CHARACTER scalar and rank-one array declarations in specialized subroutines
can use `LEN(text)` of a CHARACTER dummy, including supported constant arithmetic
such as `LEN(text)+2` or `MAX(1,LEN(text))`. Each call binds the length from the
actual argument's declared scalar or element length; the contents can be read
at runtime. Nested calls and BLOCK declarations keep their own lengths.
Specification expressions cannot depend on OPTIONAL dummies. Runtime-dependent
`LEN_TRIM(text)` lengths remain unsupported. `tests/154_dummy_character_lengths.f90`
isolates the local `mod_text` declaration from `util.f90:read_vec_real`.
Run `python tests/test_dummy_character_lengths.py` for interpreter/native O0/O2
checks and gfortran comparisons.

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
specializations. Ordinary INTEGER, REAL and LOGICAL array dummies can also use
native recursive subroutine calls with borrowed data, initialization masks and
per-dimension bounds/strides. Direct and mutual recursion, OPTIONAL presence
variants, ranks through 15, writable scalar dummies, explicit-shape bounds and
runtime-sized actual arrays are supported. Each activation has its own scalar
locals; supported initialized locals retain shared SAVE storage. Whole-array
forwarding preserves storage identity rather than converting it into a value
expression. Section actuals use the existing association/copy rules.

Array operations and sections inside these procedures use runtime descriptors.
Overlapping assignments use temporary heap storage when the extent is unknown.
Bounds and per-element initialization checks remain active, and INTENT(OUT)
invalidates previous values on every recursive entry. Explicit-shape checks use
captured scalar actuals even when the bound dummy follows the array in the
argument list. `xrecursive_optional_array.f90` reproduces the `util.f90` gen/combos
case; `tests/188_recursive_optional_array.f90` is its numbered regression.
Run `python tests/test_recursive_arrays.py` for interpreter/native O0/O2 and
gfortran comparisons.

Recursive CHARACTER, ALLOCATABLE and POINTER dummy interfaces and forwarding a
new section on a recursive edge retain their existing limitations. Recursive
functions with array dummies or array-valued results remain outside this subset.
Bounded ALLOCATABLE output recursion can still terminate during specialization, as in `xpure_recursive.f90`,
whose recursive call omits the optional bounds and selects the copy branch.

Numeric and LOGICAL ALLOCATABLE subroutine dummies support ranks 1 through 15
with INTENT(IN), INTENT(OUT) and INTENT(INOUT). The dummy aliases the caller's
heap descriptor, including its data, extents, lower bounds, allocation status
and initialization mask. This works for main-program, procedure/BLOCK and module
actuals, including forwarding through another subroutine, optional dummies and
keyword arguments. The actual must be an ALLOCATABLE variable of matching type,
kind and rank. Unallocated actuals are allowed; reading their data or extents
still reports an error.

INTENT(OUT) deallocates an allocated actual on entry, including when the callee
returns without allocating a replacement. ALLOCATE and DEALLOCATE through an
INTENT(INOUT) or INTENT(OUT) dummy update the caller. INTENT(IN) preserves
allocation status and lower bounds and cannot be passed to a writable dummy.
Allocation dimensions can depend on inputs, and outputs can exceed input sizes;
a compile-time capacity bound is no longer needed for numeric dummy outputs.
ALLOCATABLE dummy calls continue to use specialization, with recursive allocation call cycles diagnosed.
Run `python tests/test_allocatable_dummies.py`.

Fixed-length rank-one CHARACTER ALLOCATABLE outputs retain bounded
specialization. Their capacity is inferred from bounded array inputs or fixed
locals. They support ALLOCATED, SIZE, element access, scalar filling and
conformable whole-array or constant-stride section copying, including padding
and truncation when element lengths differ. Fixed-length multidimensional
CHARACTER arrays also have runtime heap storage,
including section output and copying. General CHARACTER reductions remain
outside this subset.
Growing CHARACTER output cycles without a finite bound remain diagnosed.
Run `python tests/test_recursive_allocatables.py` for recursive OPTIONAL call
coverage and interpreter/native O0/O2 comparisons with gfortran.

DEALLOCATE accepts lists of whole bounded allocatable arrays and
deferred-length scalar CHARACTER locals, including in single-line IF statements.
It clears ALLOCATED and initialization state, permits subsequent allocation or
CHARACTER assignment, and reports an error when an object is already unallocated.
Numeric locals with runtime allocation dimensions use heap storage; existing
fixed-size and output specializations may retain bounded storage. Different extents
and zero-sized allocations are supported.
Supported allocatable INTENT(INOUT) dummies share allocation state with the caller;
INTENT(IN) objects and nonlocal objects in PURE procedures cannot be deallocated.
Pointers and STAT/ERRMSG options
remain outside this DEALLOCATE subset. Run `python tests/test_deallocate.py` for
interpreter/native O0/O2 checks and comparison with gfortran.

Numeric and LOGICAL allocatable locals and module variables now support heap
allocation with runtime dimensions, including dimensions read from input or
computed by a procedure. The shared descriptor tracks allocation status, active
extents and lower bounds; contiguous column-major strides follow those extents.
Ranks 1 through 15 and INTEGER kinds 1/2/4/8 are supported. REAL and DOUBLE
PRECISION arrays also use this path. Fixed-size specializations may still reserve
bounded buffers.

SIZE (including constant DIM), SHAPE, LBOUND, UBOUND, ALLOCATED, scalar filling,
conformable array arithmetic, list-directed output, full-array reductions and
RANDOM_NUMBER use the current allocation. Explicit `lower:upper` allocation
bounds can depend on runtime values. Assumed-shape numeric dummies support
reading and writing these arrays, with dummy lower bounds rebased to one.
Each subscript is checked against its dimension. Allocation-size arithmetic is
checked for overflow, and allocation failure is reported at runtime.

DEALLOCATE releases the data and its initialization mask. Reallocation starts
with unset elements; reads remain checked per element. Module allocations persist
across procedure calls. Heap-backed procedure/BLOCK locals and expression buffers
are released on normal scope exit, including RETURN, EXIT and CYCLE.

Runtime-sized multidimensional sections support scalar and triplet subscripts,
runtime bounds and strides, omitted bounds, reverse strides and empty sections.
They work for local allocatables, automatic locals, module variables, numeric
pointers and assumed-shape dummies, with ranks through 15. Supported element types include
INTEGER, REAL, DOUBLE PRECISION, LOGICAL, COMPLEX, fixed-length CHARACTER and
supported derived types (including scalar CHARACTER components). Assignment
captures overlapping inputs before writing. Section arguments to writable
subroutine dummies use copy-in/copy-out as required by INTENT; RANDOM_NUMBER
sections do not read unset destination elements. CHARACTER section comparisons
and concatenation are lowered element by element. Bounds, shape conformance and
initialization checks remain active. Existing output restrictions for runtime
COMPLEX and whole derived-type arrays still apply. READ into runtime sections
is still outside this subset. Run `python tests/test_runtime_sections.py` for interpreter/native
O0/O2 checks against gfortran; `tests/187_runtime_sections.f90` covers the basic
regression.

The runtime section path also applies to fixed arrays: scalar selectors such as
`a(:,column)` and rank-one triplets such as `a(first:last:step)` can vary at
runtime. Specialized functions preserve these selections in nested array-valued
calls. FORALL captures its iterations before writes, and known function-result
extents are folded before native storage and output are generated. Run
`python tests/test_fixed_runtime_sections.py` for fixed-storage, overlap,
copy-in/copy-out, element-type and rank-15 regressions against gfortran.

ALLOCATABLE array dummies in functions, array transformations, masked/DIM
reductions, reallocation on assignment beyond the cases described above, and mixing fixed and
runtime-sized multidimensional expressions retain their existing limitations.
Deferred-length CHARACTER array elements and STAT/ERRMSG remain unsupported. No public descriptor ABI is provided
for separate-object module interfaces.
Zero and negative allocation extents produce allocated empty arrays.
Numeric/LOGICAL array-valued functions can use runtime-sized inputs and results
through a hidden allocated result argument, including calls nested in another
procedure's input arguments. ALLOCATABLE numeric/LOGICAL results use caller-owned
descriptors even for fixed-size actual arguments. Intrinsic assignment allocates
or resizes these results to the RHS shape, preserving RHS values before any
reallocation and retaining existing lower bounds when shapes conform. This also
supports filling a result through an ALLOCATABLE INTENT(OUT) helper. Run
`python tests/test_numeric_allocatable_results.py` to check direct assignment,
helper calls, empty/repeated results, element kinds and multidimensional ranks.
Runtime rank-one result section assignments retain
RHS values before writing. CHARACTER functions continue to use bounded storage;
direct constant allocations now supply their active shape before function calls
are materialized. `tests/184_allocated_character_concat.f90` covers the empty
CHARACTER input concatenation and allocation case from `xutil.f90`.

Derived arrays use separate component arrays with a shared shape, supporting
fixed and allocatable storage, ranks through 15, component defaults, element
access such as `x(i)%id`, constructors, assignment, and list-directed/formatted
output in element order. Array-valued functions execute once and pass their
component result arrays through a hidden subroutine call. This preserves side
effects in ordinary functions and also works for PURE functions. The current
subset supports scalar numeric, LOGICAL and fixed-length CHARACTER components;
nested/array components, polymorphic arrays, saved arrays, derived PARAMETER arrays, pointers,
finalization and a separate-object record ABI remain unsupported.
`xset_alloc_derived.f90` and `tests/185_allocated_derived_concat.f90` exercise empty
and nonempty allocated inputs and replacement of an allocated output.
Run `python tests/test_allocated_array_functions.py` for interpreter/native
O0/O2 checks and gfortran comparisons across element types.
Run `python tests/test_heap_allocations.py` and
`python tests/test_runtime_allocatables.py` for interpreter/native O0/O2 checks
and gfortran comparisons.

## CHARACTER comparisons and function arguments

Scalar CHARACTER comparisons support all six relational operators with blank
padding of the shorter operand. Fixed-length variables, ASCII literals,
concatenations, TRIM and zero-length strings are supported. Comparison captures
in DO WHILE conditions are reevaluated on every iteration, including after CYCLE.
Run `python tests/test_character_comparisons.py`.

Functions using the numeric/LOGICAL result ABI can receive fixed-length,
read-only scalar CHARACTER arguments.
Actuals may be variables, literals or fixed-length concatenations; they must be
at least as long as the dummy, and longer actuals contribute their leading
characters. Function-to-function forwarding is supported. Writable CHARACTER
function dummies, runtime-length actuals and assumed-length dummies remain
unsupported for that ABI; the CHARACTER-result specialization above supports
assumed-length scalar inputs.

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

Scalar `ICHAR`/`IACHAR` and `CHAR`/`ACHAR` are accepted in PURE procedures.
Character-code conversions work with runtime one-character substrings;
`ICHAR`/`IACHAR` require length one and return a default INTEGER. `CHAR`/`ACHAR`
accept scalar INTEGER arguments and currently support codes 1 through 127,
consistent with the non-NUL ASCII storage subset. Optional KIND arguments are
not implemented. Variable-bound scalar substring assignments snapshot the RHS,
truncate or pad it to the selected width, preserve the remaining characters,
and check bounds at runtime. Empty destination ranges perform no write.
Runtime partial updates require an initialized destination; bounded
deferred-length strings retain their allocation and length after such updates.
`tests/153_to_lower.f90` reproduces `xto_lower.f90`; run
`python tests/test_character_codes.py` for interpreter/native O0/O2 checks and
gfortran comparisons.

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
Higher ranks and the other array
transformational intrinsics remain outside this subset. Local CHARACTER
array PARAMETERs support explicit or inferred rank-one extents. Untyped constructors require equal-length CHARACTER literals
(including substituted constants). Typed `[CHARACTER(LEN=n) :: ...]` constructors
accept constant CHARACTER elements of different lengths and a constant length;
elements are padded or truncated to that length before assignment. Run
`python tests/test_character_arrays.py`.

## Vector subscripts

Arrays of ranks 1 through 15 support INTEGER vector subscripts. Implemented element
types are INTEGER, REAL, DOUBLE PRECISION, LOGICAL, CHARACTER, and derived types
with supported scalar intrinsic components. Scalar subscripts, triplets and vector
subscripts can be mixed on different dimensions; the first retained dimension
varies fastest. Selectors may be INTEGER arrays,
constructors, constant implied-DOs, sections, supported arithmetic, ABS/MOD/MODULO
or default INT conversions. Reads preserve order and repetitions. Assignments
capture all indices and right-hand values before writing, including scalar fills,
overlapping selections and an array used as its own index vector. Nondefault
lower bounds and INTEGER kinds 1/2/4/8 are supported.

Numeric and LOGICAL selections use active runtime descriptors for allocated source
arrays and index vectors, including empty vectors, without the former 1024-element
selection limit. Numeric POINTER sources and POINTER index vectors use their
active bounds and strides. CHARACTER selections use bounded storage when a small
capacity is available, or heap storage for runtime descriptor-backed sources.
Vector actual arguments are materialized for known INTENT(IN) subroutine dummies;
association with writable dummies is rejected.

Constant invalid indices and duplicate assignment indices are compile errors;
dynamic indices are checked against the original bounds before any writes, and
dynamic duplicate destinations cause a runtime error. Duplicate indices remain
valid for reads. SIZE, SHAPE, LBOUND, UBOUND, KIND and CHARACTER LEN inquiries use shape
and type information without reading the array or index values. Only selected
source elements must be initialized. Loop-condition captures are reevaluated on
every iteration, including after CYCLE. Run `python tests/test_vector_subscripts.py`
and `python tests/test_vector_ranks.py` for interpreter/native O0/O2 and gfortran checks.

Vector READ destinations and vector selections in WHERE assignments remain
unsupported. Deferred-length CHARACTER array elements remain unsupported. Derived selection
output currently requires fixed result extents. Derived types with
nested or array components are not yet implemented. Other vector expressions are rejected when they
reach scalar array-index lowering, instead of silently using their first element.

## DOT_PRODUCT

`DOT_PRODUCT(VECTOR_A, VECTOR_B)` supports rank-one arrays of the implemented
INTEGER kinds 1/2/4/8, REAL and COMPLEX kinds 4/8, including mixed numeric
types/kinds, and pairs
of LOGICAL arrays. Numeric results use multiplication followed by summation;
complex results conjugate the first operand. LOGICAL results use `ANY(a .AND. b)`.
Empty vectors return numeric zero or logical false. Keyword arguments, array
expressions, supported sections/vector selections, procedure arguments and PURE
procedures use the same lowering. Result-kind inquiries do not read array values.

The compiler rejects invalid ranks/types and statically unequal lengths before
flattening arrays. Runtime-sized arrays use the existing conformance checks.
Array-expression and storage restrictions of the reduction machinery still
apply. Run `python tests/test_dot_product.py`; the numbered suite includes
`tests/186_dot_product.f90`.

## COMPLEX

COMPLEX uses REAL component kinds 4 (default) and 8. Supported forms include
kind selectors, literals `(real,imaginary)`, `CMPLX` (including keywords),
`%re`/`%im`, scalar and array assignment, constructors and simple implied-DOs,
arithmetic, equality, integer and real/complex powers, `REAL`, `DBLE`, `AIMAG`,
`ABS`, `CONJG`, `SUM`, conjugated `DOT_PRODUCT`, and rank-one/rank-two `MATMUL`.
The implemented complex math functions are `SQRT`, `EXP`, `LOG`, `LOG10`,
`SIN`, `COS`, `TAN`, `SINH`, `COSH`, and `TANH` (`LOG10` with COMPLEX is an
extension). Ordinary complex dummy arguments
and scalar function results use the existing procedure lowering. Complex named
constants support literals, conversion, conjugation and basic arithmetic.

List-directed output prints compact `(real,imaginary)` pairs, trimming generated
buffer padding while retaining ordinary CHARACTER padding. Explicit formats consume
two real descriptors for each complex element. REAL `G0`/`G0.d` output is also
supported. `CMPLX` without KIND returns default COMPLEX; `REAL(z)` and `AIMAG(z)`
retain a complex argument's component kind. Initialization checks apply to each
component. This is an initial subset: complex input, kind 16, complex components
inside user-defined types, implicit COMPLEX declarations, persistent initialized
complex locals, and user-defined complex ELEMENTAL procedures are not implemented.
List-directed array output currently requires fixed extents.

Run `python tests/test_complex.py` after building. The seven external probes in
`tests/ofort/complex-selection.json` cover constructors, parts, magnitude,
roots, component kinds, arithmetic, dot products and matrix multiplication:

```bat
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --compiler target/debug/rfort.exe --selection tests/ofort/complex-selection.json --report target/ofort-complex.json --strict
```

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
require a known definition in the source group; external interface-only declarations
remain unsupported. Run `python tests/test_named_arguments.py`.

## Module data and procedure association

Procedures can read module named constants and share mutable module variables.
USE association supports variables, constants, procedures and derived types,
including ONLY lists, renaming, reexports and generic interface extensions.
PUBLIC/PRIVATE access and local shadowing are checked. Procedures with the same
name in different modules retain distinct identities.

INTEGER, REAL, LOGICAL and fixed-length CHARACTER data use shared storage across
calls, including supported fixed arrays and runtime-allocated numeric arrays.
Allocation status, shape and initialization checks persist across procedures.
PURE procedures can read module data but cannot modify nonlocal variables.
Existing type and rank limits still apply. Numeric module arrays use heap storage;
CHARACTER allocation retains its existing bounds. This does not add a general
module pointer or derived-type storage ABI.

Supply module sources together, for example
`rfort m1.f90 m2.f90 main.f90 --run`, or compile them separately using `-c`
and the persistent `.rmod` interfaces described above. Run
`python tests/test_module_scope.py` for interpreter, native O0/O2 and gfortran
comparisons and `python tests/test_module_compilation.py` for cross-object tests.

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
python tests/test_module_compilation.py
python tests/test_record_read.py
python tests/test_runtime_read_format.py
python tests/test_named_loops.py
python tests/test_write_controls.py
python tests/test_formatted_implied_do.py
python tests/test_empty_constructors.py
python tests/test_internal_procedures.py
python tests/test_empty_read.py
python tests/test_read_end.py
python tests/test_statement_labels.py
python tests/test_unnamed_procedure_ends.py
python tests/test_declarations_without_colons.py
python tests/test_pointer_association.py
python tests/test_pointer_descriptors.py
python tests/test_recursive_allocatables.py
python tests/test_recursive_arrays.py
python tests/test_deallocate.py
python tests/test_runtime_allocatables.py
python tests/test_heap_allocations.py
python tests/test_allocatable_dummies.py
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
python tests/test_forall_functions.py
python tests/test_forward_array_results.py
python tests/test_multirank_array_functions.py
python tests/test_character_allocatable_results.py
python tests/test_array_constructors.py
python tests/test_runtime_constructors.py
python tests/test_bound_inquiries.py
python tests/test_named_formats.py
python tests/test_runtime_formats.py
python tests/test_nonadvancing_output.py
python tests/test_elemental_setters.py
python tests/test_impure_elemental.py
python tests/test_character_elemental.py
python tests/test_character_functions.py
python tests/test_character_array_functions.py
python tests/test_source_diagnostics.py
python tests/test_do_steps.py
python tests/test_where.py
python tests/test_explicit_bounds.py
python tests/test_nested_where.py
python tests/test_masked_reductions.py
python tests/test_dim_reductions.py
python tests/test_optional_procedures.py
python tests/test_optional_elemental.py
python tests/test_character_arrays.py
python tests/test_character_array_parameters.py
python tests/test_scalar_substrings.py
python tests/test_vector_subscripts.py
python tests/test_vector_ranks.py
python tests/test_named_arguments.py
python tests/test_ofort_compat_runner.py
python tests/test_numeric_model.py
python tests/test_bit_intrinsics.py
```

The Python tests use `target/debug/rfort` (with `.exe` on Windows).
`RFORT_TEST_COMPILER` selects another build; the legacy `F90C_TEST_COMPILER`
variable also works. The Rust integration suite defaults to the debug executable
and also accepts `RFORT_TEST_COMPILER`.
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

Scalar CHARACTER function results with lengths determined by the element width
of an array dummy support fixed arrays and runtime-sized sections. Result
specifications respect dummy/local declarations that shadow module entities,
and empty-array branches are specialized before checking their element access.
`xfirst_character.f90`, `tests/203_first_tail.f90`, and
`python tests/test_first_tail.py` cover these cases and numeric empty-array
sentinels.

Scalar CHARACTER results and locals can also have content-dependent lengths,
including `SUM(LEN_TRIM(words))`. These specifications are evaluated once on
procedure entry, and their declared length stays fixed for that invocation.
Storage is bounded by known argument widths and capacities; runtime-sized
sections read only their active elements. LEN, nested assumed-length arguments,
substrings and internal WRITE retain the logical length rather than the storage
capacity. Unbounded heap inputs and unsupported specification expressions still
produce diagnostics. `xjoin_length.f90`, `tests/204_joins.f90` and
`python tests/test_joins.py` cover joins, empty inputs and runtime WRITE overflow.

`tests/205_util_families.f90` and `python tests/test_remaining_util.py` cover
runtime CHARACTER constructors with array-valued items, elemental generic
dispatch (including keywords and matching-rank precedence), allocatable output
arguments and external integer input. Materialized multidimensional expressions
retain their original rank and shape during argument association; tests cover
reallocation, empty CHARACTER RESHAPE results, PAD/ORDER and ranks through 15.

### External util.f90 procedures

Small drivers track progress toward compiling and running `kind.f90 util.f90
xutil.f90`. The suite targets 120 specific procedures in 34 drivers and
inventories all 551 procedures in the current external utility source, leaving
untested procedures visible. It compares native O0/O2 results with gfortran and
retains standalone reproducers under `target/util-repros`.

```bat
python tests/run_util_coverage.py
python tests/run_util_coverage.py --case string_matching --mode both
python tests/test_util_coverage_runner.py
```

The default isolates unchanged procedure bodies with their dependencies;
`--mode module` tests the original full module. `kind.f90`, `util.f90`, gfortran,
and a release compiler build are required. The suite provides differential
checks for the selected procedures. See
[the runner documentation](tests/util/README.md) and
[the coverage baseline](tests/util/baseline.json) for scope and results.
The combined build now compiles and executes through `xutil.f90`'s final
intentional `check_all_equal([2,3],...)` stop. This does not imply that every
utility procedure or every output has been validated; isolated drivers remain
the differential checks against gfortran.

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

### CHARACTER compatibility inventory

[character-selection.json](tests/ofort/character-selection.json) selects 84
standalone ofort cases containing CHARACTER declarations or focused string
intrinsics. Run:

```bat
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --selection tests/ofort/character-selection.json --jobs 4 --report target/character-compat.json
```

The [checkpoint](tests/ofort/character-checkpoint.json) records source hashes,
CHARACTER declaration locations in `util.f90`/`xutil.f90`, and per-case outcomes.
The first CHARACTER batch increased the focused 37-case sample from 6 to 11
passes. At that checkpoint the broader selection had 16 passes, 63 compiler failures and five
reference failures. These broader cases also exercise unsupported derived types,
procedure pointers and I/O; their failures are not all CHARACTER bugs. Major
remaining CHARACTER work includes unbounded runtime-dependent automatic string
storage, deferred-length arrays,
explicit allocation selectors/SOURCE/MOLD, allocatable CHARACTER components,
and internal array files.
The full `kind.f90 util.f90 xutil.f90` build now gets past the initialized CHARACTER
local, CHARACTER operator interface, numeric `.notin.` operator and generic
`exclude(shape(...), ...)` call. `SHAPE`, `LBOUND` and `UBOUND` now retain their
INTEGER scalar or rank-one vector types during generic dispatch, and fixed-shape
inquiry vectors can be passed to specialized read-only array functions.
`xexclude.f90` prints `2 4`; `tests/test_generic_inquiries.py` checks dispatch,
keywords, sections, empty extents and initialization diagnostics. The full build
now gets past generic `repeat` resolution: a user generic can fall back to
intrinsic `REPEAT` for scalar CHARACTER/INTEGER arguments, including the intrinsic
keywords `STRING` and `NCOPIES`. Matching user specifics take precedence.
Native `REPEAT` supports constant copy counts, CHARACTER sources and concatenation;
runtime-dependent copy counts currently produce a native compilation error.
`xrepeat_intrinsic.f90` prints `[000]`, and `tests/test_repeat_generic.py` checks
the fallback, keyword association, user overloads and CHARACTER output. The full
build now gets past the constructor-related `set_alloc` dispatch. Constructor
inference flattens scalar and array-valued elements to a rank-one type, checks
type/kind consistency, and infers implied-DO items independently of their bounds.
`tests/test_constructor_inference.py` covers numeric, LOGICAL and CHARACTER items,
nested implied-DOs, typed empty constructors and mixed scalar/array elements.
Read-only numeric/LOGICAL runtime-bound constructor actual arguments now use
runtime storage, including bounds read from input or changed by previous calls.
A compile-time capacity proof is no longer required. The constructor retains
runtime controls and an active length, preserves implied-DO index scope, and
uses checked temporary storage. Nested implied-DOs, runtime positive/negative
strides and empty results are supported. `xset_alloc_implied_do.f90` now compiles
and prints `1 2 3`;
`tests/test_constructor_arguments.py` covers this path in the interpreter and
native code at `-O0` and `-O2`. Generic dispatch now preserves array-section
rank: scalar subscripts remove
axes, while triplets and vector subscripts retain them. Resolver tests cover
ranks through 15 and the rank-three `select_tensor_real` calls in `util.f90`;
`xgeneric_sections.f90` checks generic dispatch after reductions of sections.
`RESHAPE` also retains its source element type and derives result rank from a
scalar-integer shape constructor, independently of runtime extent values.
The full build now passes both `set_alloc` type/rank blockers and currently
stops at a too-many-arguments error for `ivec`.
Intrinsic keyword interfaces are centralized in `src/named_arguments.rs`, with
coverage checks for the intrinsic catalog. Added interfaces cover scalar
conversions, CHARACTER inquiries, numeric-model/kind inquiries, array operations,
and variadic `MIN`/`MAX`, including reordered keywords and omitted optional
arguments. `KIND` uses its standard argument name `X`.
`RANDOM_SEED(SIZE=...)`, `RANDOM_SEED(PUT=...)` and `RANDOM_SEED(GET=...)` now work
in the interpreter and native code. The seed representation uses two default
INTEGER words for the existing 64-bit generator state. SIZE defines a scalar
or an array element; PUT/GET require a rank-one default INTEGER array of at least
two elements. PUT is an input argument and accepts compatible array expressions,
including arithmetic, constructors, sections and PACK; GET requires writable
storage. Expressions use the same array materialization as other procedure
inputs. `xrandom_seed_expression.f90` compares expression and variable inputs.
GET can save the current stream for restoration with PUT.
The no-argument form retains its deterministic reset. `xrandom_seed.f90` checks
stream restoration and output range; `tests/test_intrinsic_keywords.py` checks
keyword association and seed argument/intent diagnostics.
`CPU_TIME(TIME=...)` and `SYSTEM_CLOCK(COUNT=..., COUNT_RATE=..., COUNT_MAX=...)`
work in the interpreter and native code, including reordered keywords, omitted
clock outputs, scalar array elements and output dummy arguments. CPU_TIME returns
process CPU seconds in REAL kind 4 or 8; SYSTEM_CLOCK uses a monotonic clock.
INTEGER kind 4 counts use 1,000 ticks per second; kind 8 counts use 1,000,000,000.
COUNT_MAX is the largest positive integer of the selected kind, and counts wrap
to zero. Integer outputs must share a kind; COUNT_RATE can also be REAL. If the
OS clock is unavailable, CPU_TIME returns -1, and SYSTEM_CLOCK returns
-HUGE(COUNT) with zero rate and maximum. Both intrinsics are rejected in PURE
procedures. The OS adapters cover Windows, Linux and macOS; this change was tested
on Windows. `xtiming.f90` demonstrates both calls, and `tests/test_timing.py`
checks the interpreter, native code at O0/O2, and gfortran when available.
`DATE_AND_TIME` supports all four optional keywords (`DATE`, `TIME`, `ZONE`,
`VALUES`) in the interpreter and native code. One snapshot produces local date
`yyyymmdd`, time `hhmmss.sss`, UTC offset `+hhmm`/`-hhmm` and the eight integer
fields. CHARACTER outputs are truncated or padded to their declared lengths,
and scalar elements of supported CHARACTER arrays are accepted. VALUES accepts
a writable whole rank-one INTEGER array of kind 2, 4 or 8 with at least eight elements;
non-default lower bounds work, and only the first eight elements are written.
Unavailable clocks produce blank strings and negative HUGE values. PURE calls,
read-only outputs and invalid types/ranks are diagnosed. Windows, Linux and macOS
have OS adapters; validation here ran on Windows. `xdate_and_time.f90` is a small
example, and `tests/test_date_and_time.py` checks consistency and argument rules
against the interpreter, native O0/O2 and gfortran when available.
Full `xutil.f90` execution remains a separate acceptance test.

## License

The combined compiler is licensed under **[GPLv3](LICENSE)**. Original Rust
modules added in this fork and listed in [LICENSING.md](LICENSING.md) are also
available individually under the **[MIT license](LICENSE-MIT)**. Their source
files carry MIT SPDX headers. Upstream code remains subject to GPLv3.

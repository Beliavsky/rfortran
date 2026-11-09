# Using rfort

[README](../README.md) covers building rfort and the basic commands. This page
describes standard selection, warnings, multi-file builds and separate compilation of modules.

## Standard selection

Select a language revision with `-std=f90`, `-std=f95`, `-std=f2003`,
`-std=f2008`, `-std=f2018`, or `-std=f2023`. The equivalent spelling
`--std f2008` also works. Without this option rfort continues to accept all
implemented features. A revision mode does not enable unimplemented features.

```cmd
rfort -std=f90 main.f90 --run
rfort -std=f95 check main.f90
rfort -std=f2003 xtyped_constructor.f90 --run
```

The compatibility checker runs on the original AST and token spelling before
lowering. Diagnostics identify the required revision and selected revision,
with the original file and line. Native builds, interpreter runs, checks, and
AST exports use the same checker. Source groups and definitions loaded from
previously compiled `.rmod` interfaces are checked too. Existing object files
supplied for linking cannot be checked without their source.

Checks currently cover PURE/ELEMENTAL, FORALL and derived-component default
initialization (Fortran 95); square-bracket and explicitly typed constructors,
polymorphism, type-bound procedures, ASSOCIATE, deferred CHARACTER lengths,
allocatable scalars/dummies/results/components, newer declaration attributes,
intrinsic modules and some I/O features (Fortran 2003); BLOCK, DO CONCURRENT,
IMPURE prefixes, CONTIGUOUS, NEWUNIT, ERROR STOP and ranks above seven (Fortran
2008); ERROR STOP in PURE procedures (Fortran 2018); and a table of newer
intrinsics, including CPU_TIME, command/environment queries, newer bit and math
functions, RANDOM_INIT, OUT_OF_RANGE, and the Fortran 2023 degree/pi trig functions.
User-defined procedures and arrays sharing intrinsic names are distinguished.

FORALL produces an obsolescence warning in the 2018 and 2023 modes; `-Werror`
promotes it to an error. The permissive default does not enable these warnings.
Introduction, obsolescence and deletion metadata live in `src/standards.rs`.

**These are partial compatibility checks, not complete standard-conformance
validation.** Context-dependent intrinsic overloads, all extension spellings,
every obsolescent/deleted feature and every language constraint are not yet
classified. Passing `-std=f90`, for example, does not certify Fortran 90
conformance. Regression checks: `python tests/test_standards.py`.

## Run-time checks

Compiled programs stop with a message when they read an uninitialized variable,
use a subscript outside an array's bounds, or overflow checked INTEGER arithmetic,
at every optimization level.
`-nocheck` compiles without these checks; `-fast` is `-O3 -nocheck`. Without
the checks, such a read gives whatever the memory holds and such a subscript
can read or overwrite other storage, as in gfortran without `-fcheck`. Use
them for tested programs whose speed matters.

```cmd
rfort -nocheck main.f90 --run
rfort -fast main.f90 -o main.exe
```

The options affect compiled programs only: the `run` subcommand keeps its
checks. Compile-time errors for variables read before any assignment remain.
Allocation-status checks (for example, assignment to an unallocated array)
are kept. An explicit `-O` level cannot be combined with `-fast`. On the
medium benchmark profile measured before integer-overflow checks were added,
`-nocheck` made rfort about 5% faster overall
(geometric mean) and at most about 1.4 times faster, because rfort already
removes checks it can prove unnecessary (see
[compiler-performance.md](compiler-performance.md)).
Regression checks: `python tests/test_nocheck.py`.

### INTEGER arithmetic and overflow

Default INTEGER has a 32-bit signed range. Integer expression results follow
their operand kinds, including kinds 1, 2, 4 and 8, even though the native
implementation uses 64-bit storage cells. Checked addition, subtraction,
multiplication, division, exponentiation, negation, ABS, numeric INT conversions
and narrowing assignments report overflow. Diagnostics include the source line;
constant arithmetic overflow is a compilation error.

As for other operators, an integer power has the kind of the operand with the
greater range, so `10**i` is INTEGER(8) when `i` is. A wider destination does
not widen the expression assigned to it. To calculate large powers, write both
the destination and the base with the intended kind:

```fortran
use iso_fortran_env, only: int64
integer :: i
integer(int64) :: j
i = 12
j = 10_int64**i
```

With `--nocheck`, runtime integer arithmetic and narrowing assignments keep the
low-order bits of the result kind. Constant overflow is still rejected; integer
division by zero still terminates. BOZ arguments to INT retain bit-pattern
interpretation. The Fortran standard does not specify a wrapped result for
integer overflow, so programs requiring that behavior are not portable.

Regression checks: `python tests/test_integer_overflow.py`.

## Warnings

The following source advice shares the existing `-Wno-<category>`,
`-Werror=<category>`, and `!#allow(category)` / `!#deny(category)` controls.
Diagnostics identify original source locations; shadowing, dead-store, aliasing,
and lifetime diagnostics also identify the related declaration or assignment.

`-Wextra` enables selected opt-in advice that `-Wall` leaves out (including
`-Wmissing-intent` and `-Wfixed-unit`). `-Wconversion`, `-Wimplicit-interface`,
`-Wmodule-default-public`, `-Wuse-without-only`, `-Wcould-be-parameter` and `-Wliteral-kind` remain individually selectable. As in gfortran,
`-Wextra` does not imply `-Wall`; use
`-Wall -Wextra` for both. An option naming a category overrides the group in
either order, so `-Wall -Wextra -Wno-shadow` drops only shadowing advice;
`-Wno-extra` turns the group off again.

| Option | In `-Wall` | In `-Wextra` | Initial scope |
| --- | --- | --- | --- |
| `-Wshadow` | No | Yes | Local data declarations hiding enclosing variables or named constants, including USE-associated names |
| `-Wmodule-default-public` | No | No | User modules without a standalone default PRIVATE or PUBLIC statement; entity accessibility attributes and lists do not set a default |
| `-Wuse-without-only` | No | No | USE statements without ONLY for modules in the program or loaded from `.rmod` files, with the suggested ONLY list; see [USE without ONLY](#use-without-only) |
| `-Wcould-be-parameter` | No | No | Local scalars defined once from a constant expression and never modified, with the suggested PARAMETER declaration; see [Variables that could be named constants](#variables-that-could-be-named-constants) |
| `-Wliteral-kind` | No | No | Kind numbers in type specifications, literal suffixes and conversion KIND arguments, such as `real(8)`, `real*8`, `1.0_8` and `int(n, 8)`, with ISO_FORTRAN_ENV names suggested; see [Kind numbers](#kind-numbers) |
| `-Wshadow-dummy-argument` | No | Yes | The corresponding check for dummy arguments, separately selectable |
| `-Wdead-store` | No | Yes | Straight-line assignments to local scalars overwritten before a read |
| `-Winteger-division` | Yes | No | Constant integer divisions with a nonzero discarded remainder, including named constants |
| `-Wunchecked-status` | No | Yes | Local scalar IOSTAT, allocation STAT, and intrinsic status results not inspected or forwarded |
| `-Wtarget-lifetime` | Yes | No | Pointer associations escaping a known automatic local target |
| `-Waliasing` | Yes | No | Known overlapping actuals when a defined procedure's OUT dummy, or modified INOUT dummy, conflicts with another dummy's reads |
| `-Wconversion` | No | No | Potential information loss in implicit numeric conversions and CMPLX calls without KIND |
| `-Wcmplx-kind` | Yes | No | Wider REAL arguments to CMPLX without KIND; also enabled by `-Wconversion` |
| `-Wprecision-loss` | Yes | No | Lower-precision literals or calculations subsequently widened to a higher real/complex kind |
| `-Wreal-literal-kind` | No | No | Real literals without an explicit kind suffix or D exponent, including exactly representable literals such as `1.0` |
| `-Wexcess-real-digits` | Yes | No | More significant decimal digits than the literal's own REAL kind guarantees (6 for REAL(4), 15 for REAL(8)); exactly representable values are exempt |
| `-Wpointer-init` | Yes | No | Derived-type pointer components lacking default initialization |
| `-Wdo-subscript` | Yes | No | Loop-derived scalar subscript ranges extending beyond known fixed array bounds |
| `-Wenum-exhaustive` | Yes | No | SELECT CASE without CASE DEFAULT missing declared values of an enumeration type or named interoperable enum type |
| `-Wimplicit-interface` | No | No | Calls lacking a known explicit interface; module/internal procedures, interface blocks, generics and known intrinsics are exempt |
| `-Wintrinsic-shadow` | No | Yes | Defined or declared user procedures sharing a known intrinsic procedure name; ordinary data names are exempt |
| `-Wkeyword-name` | No | Yes | Variables, dummy arguments and procedures named like Fortran statement keywords or attributes, such as `block`, `exit`, `if`, `data` or `result`; legal, since Fortran has no reserved words, but harder to read. Intrinsic procedure names such as `len` are left to `-Wintrinsic-shadow` |
| `-Wunderflow` | Yes | No | Supported nonzero constant expressions or assignments rounding to zero in REAL(4)/REAL(8) |
| `-Wempty-case` | Yes | No | Reversed constant INTEGER, CHARACTER or enumeration CASE ranges |
| `-Wunused-label` | Yes | No | Unreferenced statement labels within each program unit, including FORMAT labels |
| `-Wcharacter-truncation` | Yes | No | CHARACTER values of compile-time length (literals, fixed-length variables and PARAMETERs, concatenations, constant substrings, array constructors) assigned or initialized into a shorter fixed-length variable, element, component or substring, as gfortran's `-Wcharacter-truncation`. Deferred-length destinations reallocate and are exempt; actual arguments and DATA statements are not checked |

Interface advice distinguishes optional warnings from language requirements.
Keyword calls without an explicit interface are errors. When a procedure body
is available, supported mandatory-interface cases (such as OPTIONAL or
assumed-shape dummies, ELEMENTAL procedures, or array-valued results) are also
errors, even without warning flags. `-Wno-implicit-interface` does not suppress
these errors. Unknown external interfaces get opt-in advice; this is not complete
validation of every Fortran explicit-interface requirement.

Underflow proofs cover supported constant arithmetic, selected intrinsic calls,
named constants and narrowing assignments. Nonzero subnormal values and exact
zero are exempt. Uncertain cancellation and values near halfway rounding
boundaries are skipped. Inquiry arguments are not treated as evaluated values.
These checks do not predict runtime underflow in nonconstant calculations.

Empty CASE advice compares CHARACTER constants with Fortran blank padding;
open or unresolved bounds are skipped. The diagnostic does not add native
support for CHARACTER CASE ranges, which remains a separate limitation.
Unused-label advice recognizes GO TO, labeled DO, FORMAT and supported I/O
branch references within each program unit. Dynamic format expressions suppress
unused FORMAT-label advice when use cannot be established statically.

Regression checks: `python tests/test_call_syntax_warnings.py`.

Conversion advice includes narrowing integer/real/complex kinds, real-to-integer
assignment, loss of a complex imaginary component, and integer-to-real conversion
when the real kind cannot represent the integer type's full range. Provably
exact constants, harmless widening and explicit conversions are exempt. Checks
cover supported assignments and declaration initializers, including arrays and
resolved components/function results; unknown types and kinds are skipped.
Procedure argument type mismatches remain subject to existing interface checks,
rather than being treated as implicit assignment conversions.

`-Wcmplx-kind`, enabled by `-Wall` and `-Wconversion`, diagnoses `CMPLX` calls
with wider REAL arguments and no `KIND` argument, including calls inside output
statements and array expressions.
For example, `cmplx(1.1_dp, 2.2_dp)` produces default-kind complex components even
when assigned to `complex(dp)`. Use `cmplx(1.1_dp, 2.2_dp, kind=dp)` to preserve
their precision. Explicit `KIND` arguments and provably exact constants are exempt.
Use `-Wno-cmplx-kind` to suppress this advice while retaining other conversion
warnings. An explicit `-Wcmplx-kind` or `-Wno-cmplx-kind` takes precedence over
the `-Wconversion` group in either command-line order; `-Wno-conversion` disables
the group, including this warning unless explicitly enabled separately.

Precision advice explains why `real(real64) :: x; x=sqrt(2.0)` still performs a
default-real calculation. It also recognizes inexact default-real literals in
wider expressions and work hidden inside a subsequent widening conversion.
Exactly representable cases such as `x=1.0`, `x=0.5`, and `x=sqrt(4.0)` are
suppressed, as are deliberate explicit narrowing conversions. Merely widening
a stored lower-kind variable is not diagnosed as a new calculation. This replaces
the former six-decimal-place literal heuristic; binary kind precision is used
instead. Constant proofs use the implemented integer and REAL(4)/REAL(8) models.

Literal advice is separate from precision-loss advice: it applies even without
a wider destination. `-Wreal-literal-kind` is an opt-in style rule; `3.14`
and `1.0` warn, while `3.14_wp`, `3.14_8` and `3.14d0` explicitly select a kind.
The suffix's named kind is resolved in its scope, including supported USE
association. No particular kind-parameter name is required.

`-Wexcess-real-digits`, included in `-Wall`, warns for
`3.141592653589793` because this default REAL(4) literal supplies 16 significant
digits. It also checks explicitly kinded literals against their own precision.
This is advice about decimal digits beyond the kind's guarantee, not a claim
that every extra digit is useless. Leading and trailing zeros are excluded
from the digit count. Exact binary values, including `16777216.0`, `1.234375`
and long exact dyadic decimals, are exempt; the proof compares decimal text
with the exact stored binary value rather than rounding both texts to host
double precision first. Unsupported or unresolved kinds are skipped.
Both warnings highlight the literal itself, ignore comments/strings/FORMAT
descriptors, and support the usual `-Wno-...`, `-Werror=...` and source controls.
An explicitly selected style warning and the existing precision-loss warning
may accompany an excess-digit warning, since they describe separate properties.
Regression checks: `python tests/test_real_literal_warnings.py`.

Pointer initialization advice checks each component individually, including
pointer arrays and pointers to derived types. `=> NULL()` and other explicit
default initializers suppress it; ALLOCATABLE components and ordinary local
pointers are outside this category. This diagnoses a risky declaration pattern,
not proof that every object is subsequently used without initialization.

Loop-subscript advice uses fixed declared bounds of any supported rank, constant
or enclosing-loop-derived DO ranges, positive/negative strides, simple affine
subscripts, and recognized IF guards (including early CYCLE/EXIT/RETURN guards).
It checks reads and writes and uses the last reachable value for constant-stride
loops. Guard facts apply to the guarded body, without assuming `.AND.` or `.OR.`
short-circuit evaluation. Unknown bounds, descriptors, general vector/section
subscripts and relationships beyond interval analysis are not proved. Calls and
assignments invalidate affected facts conservatively. A reported possible range
does not prove the access executes on every path; existing constant-bound errors
and runtime checks remain in place.

Enumeration coverage lists missing enumerators, respects USE/host association
and renamed types/values, handles case ranges and aliases sharing a value, and
accepts CASE DEFAULT. Unrestricted integer selectors are exempt. Coverage of
declared enum values is not a proof that every underlying interoperable integer
value is covered. Unknown case values suppress this advisory check and leave
validation to the existing enumeration/type checks.

Dead-store analysis excludes dummy arguments, function results, module variables,
saved data, pointers, targets, allocatables, volatile/asynchronous data and
storage-associated variables. Calls and control-flow boundaries stop the
straight-line analysis. It does not diagnose final assignments merely because
the variable is not subsequently read.

Unchecked-status advice recognizes READ/WRITE and file-control IOSTAT,
ALLOCATE/DEALLOCATE STAT, and status outputs described by implemented intrinsic
signatures. It also recognizes EXECUTE_COMMAND_LINE's EXITSTAT/CMDSTAT, including
where native command keyword lowering remains unsupported. A later read or
forwarding suppresses the warning; this does not prove correct error handling.
Possible inspection in a branch, loop, or opaque call is treated conservatively.
Observable output storage (including dummy arguments and saved/module data) and
array elements/components are outside this initial check.

Lifetime analysis follows direct local pointer aliases within a straight-line
region and recognizes pointer results, output dummies, saved/module pointers,
and pointers in enclosing scopes. It excludes unknown dummy-target lifetimes;
it does not implement general pointer-component escape analysis. Aliasing
analysis uses defined procedure interfaces and keyword association, whole
variables, constant elements, and unit-stride section overlap. It skips unresolved interfaces,
unknown subscript relationships, and POINTER/TARGET/VALUE dummy exceptions.
Neither check is a complete proof of storage safety or Fortran conformance.

Enable all the new advice with, for example:

```cmd
rfort -Wall -Wextra -Wconversion -Wimplicit-interface check example.f90
```

Regression checks: `python tests/test_source_advice.py` and
`python tests/test_value_warnings.py` (after building the debug compiler, or with
`RFORT_TEST_COMPILER` set to the desired executable).

`-Wshort-circuit`, enabled by `-Wall`/`--wall`, warns when a logical expression
combines an intrinsic `ALLOCATED` or `ASSOCIATED` inquiry with an access that
needs valid allocation or association. For example,
`if (allocated(x) .and. size(x)>0)` is unsafe unless allocation is already
established: Fortran `.AND.` and `.OR.` do not guarantee short-circuit evaluation.
The check covers scalar/array pointer values, sections, shape inquiries, and
component accesses. It suggests nested IF statements or lazy conditional
expressions, and highlights a value reference rather than the safety inquiry
where possible. `PRESENT`-related checks remain in `-Woptional-argument`.

Prior positive/negated guards, early returns, branch joins, successful explicit
allocation without STAT, and completed intrinsic assignments are considered.
Proofs are reset conservatively across calls that could change storage and
loop-carried changes. The check recognizes non-evaluating type/rank inquiries,
non-deferred CHARACTER length inquiries, and whole-descriptor forwarding to
known ALLOCATABLE, POINTER, or OPTIONAL dummy arguments. It respects local and
USE-associated procedure names that shadow intrinsics, block-local declarations,
and provably unreachable code. It does not infer arbitrary caller contracts,
logical-variable aliases for guards, or allocation success from a STAT result.
This is advice about guards, not a replacement for allocation, association, or
uninitialized-read checks; the inquiry itself must still be valid.

Disable with `-Wno-short-circuit`, promote with `-Werror=short-circuit`, or use
`!#allow(short-circuit)` / `!#deny(short-circuit)`. Diagnostics retain original
multi-source locations, and loaded `.rmod` definitions are not diagnosed again.
Example: `rfort -Wall tests/programs/xshort_circuit_warnings.f90 --run`.
Regression checks: `python tests/test_short_circuit_warnings.py`.

`-Wimplicit-save`, enabled by `-Wall`/`--wall`, warns when declaration
initialization gives a procedure-local variable implicit `SAVE`. For example,
`integer :: count=0` initializes persistent storage, rather than resetting
`count` on each call. The warning includes array and pointer initializers, and
exempts explicit `SAVE` (including separate and bare SAVE statements), named
constants, module/main-program variables, and derived-type component defaults.
It points to the original entity name and suggests either documenting persistent
state with explicit `SAVE`, or using executable initialization on each invocation.
This diagnostic does not expand existing backend support for saved pointers.
Disable with `-Wno-implicit-save`, promote with `-Werror=implicit-save`, or use
`!#allow(implicit-save)` / `!#deny(implicit-save)`.

`-Wmissing-intent` is opt-in and is **not enabled by `-Wall`**; `-Wextra` enables it. It warns about
data dummy arguments without explicit `INTENT`, without choosing an intent or
rewriting the procedure. Procedure, POINTER, and VALUE dummies are exempt in
this initial implementation. Separate INTENT statements and derived-type dummy
attributes are recognized. The note explains that `INTENT(OUT)` makes the
argument undefined on entry, so it must not be added merely to silence a warning.
Disable with `-Wno-missing-intent`, promote with `-Werror=missing-intent`, or use
`!#allow(missing-intent)` / `!#deny(missing-intent)`.
Both checks run before lowering, including on uncalled procedure definitions,
and retain original file locations in multi-source builds. Definitions loaded
from `.rmod` files are not diagnosed again. Example:
`rfort -Wall -Wmissing-intent check tests/programs/xdeclaration_warnings.f90`
(or `-Wall -Wextra`).
Regression checks: `python tests/test_declaration_warnings.py`.

`-Wformat`, enabled by `-Wall`/`--wall`, checks known formats against known
I/O-list types for formatted PRINT, WRITE and READ, including internal I/O.
For example, `print '(2i4)',1,1.1` warns that the second I4 expects INTEGER
but receives REAL. Literal formats, CHARACTER named constants and labelled
FORMAT statements are checked using the existing format parser and constant
evaluation. Fixed arrays, sections, bounded implied-DOs and the two real
components of COMPLEX items are considered. Reversion and unused trailing
descriptors are valid; repeated identical mismatches within an array item
produce one warning. Runtime formats and uncertain list lengths retain runtime
checks; static checking stops where descriptor alignment becomes uncertain.
Derived-type I/O is not checked yet. BOZ editing's legal noninteger types are
not diagnosed as type mismatches, even where runtime support remains limited.
Disable with `-Wno-format`, promote with `-Werror=format`, or use
`!#allow(format)` / `!#deny(format)`. Run `python tests/test_format_warnings.py`.

`-Wunreachable-code`, enabled by `-Wall`/`--wall`, warns once at the first
statement of each provably unreachable sequence. This includes statements after
`STOP`, `ERROR STOP`, `RETURN`, `EXIT` or `CYCLE`, branches whose conditions are
constant expressions, and code after an `IF` whose branches both transfer
control. Named constants and host/module association are considered; values
assigned to ordinary variables are not assumed constant. Labels conservatively
allow another entry point. Declarations and procedure definitions are excluded.
Disable with `-Wno-unreachable-code`, promote with `-Werror=unreachable-code`,
or use `!#allow(unreachable-code)` / `!#deny(unreachable-code)`.
Unused-entity checks ignore references from these unreachable statements.
Run `python tests/test_reachability_warnings.py` for regression checks.

`-Wimplicit-typing` warns once per implicitly typed variable at its first use,
including loop indices and dummy arguments without explicit types. For example,
`i=3` without a declaration warns that `i` is implicitly typed as `INTEGER(4)`.
The warning is enabled by `-Wall`/`--wall`; suppress it with
`-Wno-implicit-typing` or promote it with `-Werror=implicit-typing`.
Custom `IMPLICIT` mappings are respected. Implicit typing remains valid Fortran;
use `IMPLICIT NONE` to require explicit declarations. Source directives
`!#allow(implicit-typing)` and `!#deny(implicit-typing)` also work.
Run `python tests/test_implicit_warnings.py` for regression checks.

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
`tests/programs/xunused.f90` demonstrates these diagnostics; run `rfort -Wall tests/programs/xunused.f90` or
`python tests/test_unused_warnings.py` for the regression checks.

`-Wimplicit-open` is **on by default**. A READ or WRITE on an unconnected unit
1..1023 (other than the preconnected 0, 5 and 6) connects it to the file
`fort.N` in the working directory, as gfortran and ifx do: the file is created
if absent and positioned at its start, so a READ of a new unit reaches end of
file, and a WRITE after CLOSE starts the file again. This is a common extension,
not portable Fortran, so rfort warns at the first READ or WRITE of each constant
unit (an integer literal or a named constant) that no OPEN statement of the
program names. NEWUNIT= opens don't count, and an OPEN of a unit given by a
variable turns the check off, since it might connect any unit. Disable with
`-Wno-implicit-open` or `!#allow(implicit-open)`; promote with
`-Werror=implicit-open`. `-std=` does not change the behavior. Regression
checks: `python tests/test_implicit_open.py`.

`-Wmemory-leak` is **on by default**. It warns when storage allocated through a
local POINTER of a procedure is lost without DEALLOCATE: when the procedure
returns, or when ALLOCATE, NULLIFY or `=>` reassigns the pointer. A leak on
every path "is not deallocated"; a leak on some paths "may not be deallocated",
and names the IF whose outcome keeps the storage when there is one. When the
pointer is never used as a pointer (no `=>`, NULLIFY or ASSOCIATED) and every
leak is at a return, the warning suggests declaring it ALLOCATABLE, which is
deallocated automatically when the procedure returns. The ELSE
branch of `IF (ASSOCIATED(p))` holds no storage, so
`if (associated(p)) deallocate(p)` is not reported. A pointer that might keep
its storage elsewhere is not checked: one passed to a procedure other than a
known intrinsic, used as the target of `=>`, returned, or named in an internal
procedure. Dummy arguments, function results, SAVE pointers (including those
initialized with `=> NULL()`), ALLOCATE with STAT= and procedures with GO TO
are skipped. Disable with `-Wno-memory-leak` or `!#allow(memory-leak)`;
promote with `-Werror=memory-leak`. Regression checks:
`python tests/test_memory_leak.py`.

`-Wallocation-state` is **on by default**. It warns about DEALLOCATE of a local
ALLOCATABLE that is unallocated on every path reaching the statement, and
ALLOCATE of one that is allocated on every path, when the statement has no
STAT=; either stops the program with an error when it runs. Intrinsic,
derived-type and polymorphic ALLOCATABLEs are checked, and every form of
ALLOCATE counts, such as `allocate(x(0:n))`, `allocate(real :: x(n))` and
`allocate(character(len=n) :: s)`. A local
ALLOCATABLE starts unallocated; ALLOCATE and assignment to the whole variable
allocate it, DEALLOCATE and MOVE_ALLOC (as FROM) leave it unallocated, and the
branches of `IF (ALLOCATED(x))` know its state. A state that depends on a
condition the analysis cannot follow is not reported, so
`if (n > 0) allocate(x(n))` followed by `if (n > 0) deallocate(x)` is accepted.
Passing the whole variable to a procedure other than a known intrinsic makes
its state unknown. Variables named in an internal procedure, dummy arguments,
function results, SAVE variables and procedures with GO TO are skipped.
Disable with `-Wno-allocation-state` or `!#allow(allocation-state)`; promote
with `-Werror=allocation-state`. Regression checks:
`python tests/test_allocation_state.py`.

`-Waction-mismatch` is **on by default**. It warns about a READ from a unit
opened with `ACTION='WRITE'`, or a WRITE to one opened with `ACTION='READ'`,
when that action holds on every path reaching the statement; either stops the
program with an error when it runs. Units are integer literals or local named
constants. An OPEN with a constant ACTION of READ or WRITE and without IOSTAT=
gives the unit its action; CLOSE, another OPEN of the unit, an OPEN of a unit
given by a variable, and a CALL of a procedure other than a known intrinsic
make it unknown, and an action that depends on a branch is not reported.
Transfers with IOSTAT= are not reported, since they handle the error, and
procedures with GO TO or with ERR= or END= branches are skipped. Disable with
`-Wno-action-mismatch` or `!#allow(action-mismatch)`; promote with
`-Werror=action-mismatch`. Regression checks:
`python tests/test_action_mismatch.py`.

`-Wfixed-unit` is opt-in and enabled by `-Wextra`. It warns about an OPEN of a
constant unit (an integer literal or a named constant) in a procedure or
module, since a fixed number may already be connected elsewhere in the
program. A main program owns its units and is exempt; its internal
procedures are not. Units 0, 5 and 6 are exempt. When another program unit
opens the same constant unit, the warning names it. The note suggests
`open(newunit=u, ...)`, which connects a free unit, unless `-std=` selects a
revision before Fortran 2008. Disable with `-Wno-fixed-unit`, promote with
`-Werror=fixed-unit`, or use `!#allow(fixed-unit)` / `!#deny(fixed-unit)`.

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

## Module accessibility defaults

Fortran's standard default is PUBLIC. Opt in to private module interfaces with
`--default-private`:

```cmd
rfort --default-private main.f90 --run
rfort --default-private -c utilities.f90
rfort -Wmodule-default-public check utilities.f90
```

For each module defined in the current source group, entities default to PRIVATE
unless the module contains a standalone `public` or `private` statement.
Explicit exports such as `public :: solve` and `integer, public :: count` still
work. A standalone `public` restores the public default; explicit per-entity
PRIVATE declarations remain private. The option does not change derived-type
component or binding defaults, or intrinsic modules.

The effective accessibility is saved in `.rmod` interfaces. A module compiled
with this option keeps that policy when a caller is compiled without it.
Conversely, enabling the option for a caller does not change imported modules
previously compiled with a public default. Recompile a module to change its
default policy.

For portable source, prefer writing `private` in each module and listing its
public interface explicitly. `-Wmodule-default-public` helps enforce an explicit
source policy without changing accessibility: it warns once per user module
lacking a standalone PRIVATE or PUBLIC statement, even with `--default-private`.
Entity lists or access attributes do not suppress it. The warning is opt-in,
outside both `-Wall` and `-Wextra`, and accepts the usual `-Wno-...`,
`-Werror=...`, and source-directive controls. Neither option edits source files.

Regression checks: `python tests/test_module_default_private.py`.

## USE without ONLY

`-Wuse-without-only` reports each USE statement without an ONLY list and
suggests the list that the scope needs:

```cmd
rfort -Wuse-without-only check main.f90
```

```
warning: USE of vectors without ONLY imports all 5 of its public entities [use-without-only]
  = suggestion: use vectors, only: norm, vec, zero, operator(+), operator(.dot.)
```

A USE from which nothing is used is reported with the advice to remove it.

How the list is worked out:

- **What counts as used:** an entity is needed when its name appears in the
  scope, including in procedures it contains, which inherit the USE by host
  association. Kind parameters of literals such as `1.0_dp` count.
- **Operators and assignment:** a defined operator is needed where it is
  applied. An intrinsic operator such as `operator(+)` is needed wherever that
  operator appears, `assignment(=)` wherever there is an assignment, and defined
  input/output wherever there is input or output.
- **Never too short:** a name that appears for another reason, such as a
  component or keyword argument with the same spelling, is kept. The suggested
  list can be longer than necessary but never omits a needed entity.
- **Renames:** a renaming such as `use stats, average => mean` is kept in the
  list as `average => mean`.
- **Re-exports:** a module with a public default re-exports what it uses, so
  the names its users in the program obtain through it are included. A note
  says that users outside the program are not taken into account. A module
  containing only USE and accessibility statements re-exports on purpose and is
  not reported.
- **Scope of the check:** modules defined in the source group or loaded from
  `.rmod` files are checked. Intrinsic modules such as `iso_fortran_env` are
  not.

The warning is opt-in, outside both `-Wall` and `-Wextra`, and accepts the usual
`-Wno-...`, `-Werror=...`, and source-directive controls. It does not edit
source files; the suggested statement can replace the original line.

Regression checks: `python tests/test_use_without_only.py`.

## Variables that could be named constants

`-Wcould-be-parameter` reports a local variable that is set once, from a
constant expression, and never modified, and suggests the PARAMETER
declaration:

```cmd
rfort -Wcould-be-parameter check xarea.f90
```

```
warning: `pi` is assigned once, from a constant, and never modified; it could be a named constant [could-be-parameter]
  = suggestion: real, parameter :: pi = 3.14
  = and remove the assignment
```

The rules follow the xparam.py checker:

- **Which variables:** locals of intrinsic type in a main program or
  procedure, without attributes, that are not dummy arguments or function
  results. Arrays need a constant shape.
- **One definition:** an initialization in the declaration, or one assignment
  at the top level of the executable part, outside IF, loops, SELECT and BLOCK,
  before any other reference to the variable. An array may instead be defined
  by element and section assignments with constant subscripts, such as
  `p(:,2) = [...]` or `m(1) = 5`, that together set every element once; the
  suggestion gathers the values in array element order, with RESHAPE for rank
  two or more.
- **A constant value:** literals, named constants, intrinsic operators,
  intrinsics allowed in constant expressions such as `acos(-1.0)` or
  `kind(1d0)`, and other variables this check accepts, so `x2 = 2*x1` qualifies
  when `x1` does.
- **Nothing else defines it.** These count as definitions:
  - READ and DO variables;
  - IOSTAT=, IOMSG= and SIZE=, ALLOCATE's STAT= and ERRMSG=, and an internal
    file of a WRITE;
  - an actual argument whose dummy is not known to be INTENT(IN) or VALUE;
  - an assignment in an internal procedure;
  - any statement the analysis does not recognize, such as DATA or NAMELIST.

When the declaration also declares other variables, a note says to declare the
variable in a separate statement. When the value uses a named constant defined
after the declaration, for example by a later PARAMETER statement, a note says
to place the new declaration after that definition. The warning is opt-in, outside both `-Wall`
and `-Wextra`, and accepts the usual `-Wno-...`, `-Werror=...`, and
source-directive controls. It does not edit source files.

Regression checks: `python tests/test_could_be_parameter.py`.

## Kind numbers

Kind numbers are processor-dependent: gfortran, ifx and rfort use sizes in
bytes, so `real(8)` is double precision, but NAG by default numbers kinds 1, 2
and 3. `-Wliteral-kind` reports kinds given by a number and suggests the
ISO_FORTRAN_ENV name with the same meaning in rfort:

```cmd
rfort -Wliteral-kind check nbody.f90
```

```
warning: kind number in `real(8)` is processor-dependent [literal-kind]
12 │   real(8), parameter :: tstep = 0.01d0
   │   ^^^^^^^ use real(real64)
   = the names come from `use iso_fortran_env, only: real64`
```

| Written | Suggested |
| --- | --- |
| `real(4)`, `real(8)`, `real(16)`, `real(kind=8)`, `real*8` | `real(real32)`, `real(real64)`, `real(real128)` |
| `complex(8)`, `complex*16` | `complex(real64)` |
| `integer(1)` to `integer(8)`, `integer*4` | `integer(int8)` to `integer(int64)` |
| `1.0_8`, `5_8` | `1.0_real64`, `5_int64` |
| `real(n, 8)`, `int(x, kind=8)`, `cmplx(a, b, 8)` | `real64`, `int64` as the KIND argument |

Kinds without an ISO_FORTRAN_ENV name, such as `logical(1)` or `integer(16)`,
are reported with the advice to use a named kind. A named constant such as
`integer, parameter :: dp = kind(1.0d0)`, or `selected_real_kind(15, 307)` for
a minimum precision, is an alternative. `DOUBLE PRECISION`, named kinds and
`real(8)` as a conversion of the value 8 are not reported. Each statement gets
one warning listing its kind numbers.

The warning is opt-in, outside both `-Wall` and `-Wextra`, and accepts the usual
`-Wno-...`, `-Werror=...`, and source-directive controls. It does not edit
source files.

Regression checks: `python tests/test_literal_kind.py`.

## Multiple source files and modules

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
### Submodules

A submodule is compiled together with its ancestor module: put both in one
file, or list their files in one source group, parents before their
descendants, for example `rfort shapes.f90 shapes_impl.f90 main.f90`. Each
submodule is merged into its module before compilation. A separate module
procedure body becomes a module procedure, and the submodule's own variables,
types and procedures become private module entities, so code that uses the
module cannot see them. A body may repeat its interface
(`module function f(x) ... end function`) or inherit it
(`module procedure f ... end procedure`); the dummy argument names of a
repeated interface must match. Separate interfaces may also be declared in a
generic interface block or in a submodule, and a body may be in the module
itself.

Not yet supported: compiling a submodule apart from its module (a module whose
separate procedures have no body in the source group is an error); a submodule
entity with the same name as an entity of its module or of another submodule;
and a submodule that uses a module which uses its ancestor, since merging
makes that a circular module dependency. Run `python tests/test_submodules.py`
for the regressions.


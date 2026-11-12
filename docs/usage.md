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
| `-Wdefault-real` | No | No | REAL and COMPLEX declarations without a kind; see [Default REAL declarations](#default-real-declarations) |
| `-Wshadow-dummy-argument` | No | Yes | The corresponding check for dummy arguments, separately selectable |
| `-Wdead-store` | No | Yes | Straight-line assignments to local scalars overwritten before a read |
| `-Winteger-division` | Yes | No | Constant integer divisions with a nonzero discarded remainder, including named constants, and integer quotients converted to REAL or COMPLEX, such as `x = n/m` with REAL `x` or `1/n*x`; `real(n/m)` marks truncation as intended |
| `-Winteger-division-all` | No | No | Every other integer division, including `real(n/m)`, which truncates toward zero (`-7/2` is `-3`, while Python's `-7//2` is `-4`); for code ported from or to Python |
| `-Wdimension-override` | No | Yes | An entity's own shape or length overriding DIMENSION(...) or the CHARACTER length of its declaration, as `y` in `real, dimension(2) :: x, y(3)`, with the statement rewritten as a suggestion |
| `-Windentation` | No | No | Free-form construct bodies not indented past their opening statement, statements of one body in different columns, and END, ELSE and CASE statements not aligned with the construct; see [Indentation](#indentation) |
| `-Wearly-return` | No | No | RETURN statements after which statements of the procedure can still run, such as `if (n <= 0) return` or a RETURN in a loop; a final RETURN is not reported; see [Early RETURN](#early-return) |
| `-Wcompare-reals` | No | Yes | `==` and `/=` with a REAL or COMPLEX operand, except comparisons with an exact zero |
| `-Wlogical-comparison` | No | Yes | Comparisons with `.true.` or `.false.`, such as `flag == .true.`, with the LOGICAL value or its `.not.` suggested; also in `-Wstyle` |
| `-Wstyle` | No | No | The style group below; see [Style warnings](#style-warnings) |
| `-Wold-relational` | No | No | `.lt.`, `.le.`, `.eq.`, `.ne.`, `.gt.` and `.ge.`, with the statement rewritten using `<`, `<=`, `==`, `/=`, `>` and `>=` |
| `-Wmissing-double-colon` | No | No | Type declarations without `::`, such as `integer i, j` |
| `-Wlabeled-do` | No | No | DO loops ended by a labeled statement, such as `do 10 i = 1, n ... 10 continue` |
| `-Wdouble-precision` | No | No | DOUBLE PRECISION and DOUBLE COMPLEX, with `real(dp)` and `complex(dp)` suggested |
| `-Wbare-end` | No | No | END statements of programs, modules, procedures and derived types without the name, such as `end` or `end subroutine` |
| `-Wkeyword-case` | No | No | Keywords written in more than one case in a file, in one warning per file |
| `-Wtabs` | No | No | Tab characters in free-form code, in one warning per file |
| `-Wline-length[=N]` | No | No | Free-form lines longer than 132 characters, or N, in one warning per file |
| `-Wsemicolon` | No | No | Lines with more than one statement, such as `x = 1; y = 2`, and free-form lines ending in a semicolon; a CASE, TYPE IS, CLASS IS or RANK selector followed by one statement, such as `case ("cos"); y = cos(x)`, is not reported; in one warning per file |
| `-Wtrailing-whitespace` | No | No | Free-form lines ending in spaces or tabs, in one warning per file |
| `-Wunnamed-exit` | No | No | EXIT and CYCLE without a construct name, in one warning per DO loop |
| `-Wgoto` | No | No | GO TO statements |
| `-Wobsolescent` | No | No | Obsolescent and deleted features: computed and assigned GO TO, arithmetic IF, ASSIGN, COMMON, EQUIVALENCE, ENTRY, BLOCK DATA, FORALL, PAUSE, CHARACTER*length and alternate returns |
| `-Wstop-in-procedure` | No | No | STOP in a function or subroutine; ERROR STOP is not reported |
| `-Wassumed-size` | No | No | Assumed-size dummy arrays, such as `x(*)` or `y(n,*)`, outside BIND(C) procedures |
| `-Wredundant-implicit` | No | No | IMPLICIT NONE in a module or internal procedure, or a submodule, whose host's IMPLICIT NONE already applies; interface bodies, which do not inherit, are not reported |
| `-Wname-case` | No | No | A name written in a case different from its declaration, or from its first appearance, such as `X` for a variable declared `x`; in one warning per name |
| `-Wduplicate-constant` | No | No | REAL and COMPLEX named constants, and kind parameters such as `dp`, defined in more than one scope, such as `pi` in several procedures or modules; definitions with different values, such as `3.14159` and `3.141592653589793_dp`, are reported as such |
| `-Wdeclaration-order` | No | No | Procedures that declare a dummy argument after a local variable or the function result, or the result after a local, and declarations that mix the groups, such as `real :: x, tmp` with dummy `x`; a local needed in a dummy's or the result's declaration, such as a named constant used as a bound or kind, may come first |
| `-Wargument-order` | No | No | Argument lists with a required INTENT(IN) argument after an INTENT(OUT) or INTENT(INOUT) one, or a required argument after an OPTIONAL one, with the reordered list suggested; see [Style warnings](#style-warnings) |
| `-Wdo-while-true` | No | No | `do while (.true.)`, an endless loop, with `do` suggested |
| `-Wfunction-result` | No | No | FUNCTION statements with the result type in the prefix, such as `real function f(x)`, or without a RESULT clause, with the preferred `function f(x) result(y)` form suggested |
| `-Wcould-be-conditional` | No | No | IF blocks whose two or three branches each assign one value to the same variable, with a Fortran 2023 conditional expression suggested, such as `y = (x > 0 ? sqrt(x) : -1.0)` |
| `-Wcould-be-one-line-if` | No | No | IF blocks holding a single action statement, with the one-line IF suggested when it fits the line length, such as `if (x > 0) x = sqrt(x)` |
| `-Wone-line-if` | No | No | The reverse: one-line IFs, with the block form suggested, for projects that always use block IFs |
| `-Wunchecked-status` | No | Yes | Local scalar IOSTAT, allocation STAT, and intrinsic status results not inspected or forwarded |
| `-Wtarget-lifetime` | Yes | No | Pointer associations escaping a known automatic local target |
| `-Winfinite-loop` | Yes | No | DO loops without loop control, or `DO WHILE (.true.)`, that nothing in their body can end |
| `-Wloop-runs-once` | Yes | No | DO loops whose body cannot start a second iteration, because every path ends in EXIT, RETURN or STOP |
| `-Wreal-do-variable` | Yes | No | DO loops with a REAL variable, a feature deleted from Fortran 95 |
| `-Wredundant-cycle` | No | Yes | CYCLE as the last statement of a loop body, directly or at the end of an IF or CASE branch that ends it |
| `-Wmaybe-uninitialized` | No | Yes | Local scalars read where some path from the start of the procedure reaches the read without assigning them; see [Uninitialized variables](#uninitialized-variables) |
| `-Wintent-out-unset` | No | Yes | INTENT(OUT) dummy arguments that some path to the end of the procedure, or to a RETURN, does not assign; one never assigned is reported as "was never assigned" by default |
| `-Wcould-be-intent-out` | No | Yes | INTENT(INOUT) scalar dummy arguments that every path assigns before anything reads them or passes them to a procedure |
| `-Wunchecked-conformance` | No | Yes | Procedures that combine two assumed-shape dummies in a way that requires a size relationship, such as `x*y`, `dot_product(x, y)`, `matmul(a, b)` or `y = x`, and never compare their sizes; see [Array conformance](#array-conformance) |
| `-Wconformance` | Yes | No | Calls whose actual arguments, of sizes known at compile time, break such a relationship, as `sumproduct([1.0, 2.0], [3.0])` |
| `-Wmaybe-nonconforming` | No | Yes | Elemental operations on arrays whose sizes, from PACK or ALLOCATE, may differ, as `x1 - x2` for two PACK results; see [Array conformance](#array-conformance) |
| `-Wmaybe-out-of-bounds` | No | Yes | Constant subscripts of PACK results, which may have fewer elements, as `x1(1)` when the mask selects nothing |
| `-Wconstant-condition` | No | Yes | Conditions, or parts of them, whose values are known at compile time, such as `.true. .or. (x > y)`, `n == n` or `n > 5 .and. n < 3`; see [Constant conditions](#constant-conditions) |
| `-Wconstant-condition-all` | No | No | The same, also for values that come from named constants, such as `debug .or. n > 2` with a PARAMETER `debug` |
| `-Wcould-be-intent-in` | No | Yes | INTENT(INOUT) dummy arguments that nothing in the procedure can modify; POINTER, TARGET and polymorphic dummies are not reported |
| `-Wunused-only` | No | Yes | Names in a USE ONLY list that the scope never uses, with the shortened ONLY list suggested |
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
components of COMPLEX items are considered. A mismatch that format reversion
causes says so, as in `print '(a, f8.3)', 'x =', x(1:2)`, where the second
REAL value meets `A`; each list item gets one warning. Runtime formats and
other lists of uncertain length retain runtime checks; static checking stops
where descriptor alignment becomes uncertain.
Derived-type I/O is not checked yet. BOZ editing's legal noninteger types are
not diagnosed as type mismatches, even where runtime support remains limited.
Disable with `-Wno-format`, promote with `-Werror=format`, or use
`!#allow(format)` / `!#deny(format)`. Run `python tests/test_format_warnings.py`.

`-Wformat` also checks lists with arrays of unknown size, such as
assumed-shape or allocatable arrays, which may fit the format for some sizes
and not others:

```fortran
subroutine sub(x)
real, intent(in) :: x(:)
print "(f0.4,1x,i0)", x, size(x)
end subroutine sub
```

```
warning: the format matches the output list only when `x` has 1 element; with 2 elements in `x`, output item 1 (element 2) is REAL but meets I0 [format]
6 │ print "(f0.4,1x,i0)", x, size(x)
  = put the array last, in an unlimited group, as in `(i0,*(1x,f0.4))` for `size(x), x`, or write it in a statement of its own
  = sizes up to 6 were tried
```

With two elements, `x(2)` meets `I0` and the program fails at run time; with
none, `size(x)` meets `F0.4`. The check tries each size from 0 up to about
twice the number of data edit descriptors in the format, past which the
pattern of matches repeats, and reports the sizes that fit:

- "only when `x` has 1 element", or "has 0 or 1 elements", for a few sizes;
- "for no nonempty size of `x`", when only an empty array fits, as in
  `print "(i4)", x` for a REAL `x`;
- "for no size of `x`", when no size fits, as in `print "(a)", c, size(c)`;
- "only for some sizes of `z` and `x`", when the list has several such
  arrays.

The message gives the smallest failing sizes as an example. Formats that fit
every size are not reported, such as `(i0,*(1x,f0.4))` for `size(x), x`,
`(*(g0,1x))`, and `(f8.3)`, which writes one value per record. The check
applies when every other item has a known length and the list has at most
three arrays of unknown size, which may be whole arrays or sections; a whole
array named twice has one size. A vector subscript, as in `x([1, 3])`, makes
the length unknown, and the statement is not checked. Mismatches among the
items before the first such array are reported as usual, once.

Two further checks of the same formats are in `-Wextra`. Each needs the
number of list items to be known at compile time.

- `-Wformat-reversion` reports a list with more items than the format has
  data edit descriptors, such as `print '(f0.4)', x(1:3)`, which writes three
  records. A format whose descriptors or groups have a repeat count, such as
  `(10f8.3)`, is taken to revert on purpose, and an unlimited repeat such as
  `(*(g0,1x))` never reverts.
- `-Wformat-unused` reports data edit descriptors that no item reaches, such
  as the `f8.4` of `print '(i6, f8.4)', 123`, which often means a missing
  item, with the shortened format suggested. String and control edit
  descriptors before the first unused one are kept in the suggestion, since
  they are still written. A partly used repeat count, as in `(10f8.3)` with
  three items, descriptors after a colon edit, unlimited repeats and empty
  lists are not reported. A named constant or FORMAT statement is checked
  only when a single statement uses it, since another statement may need the
  whole format.

`-Wcould-be-at` suggests the Fortran 2023 AT edit descriptor, which writes a
CHARACTER value without its trailing blanks, in place of `trim`:

```
warning: the AT edit descriptor can replace `trim` in this output list [could-be-at]
5 │ print "(*(1x,a))", (trim(s(i)), i=1,3)
  = suggestion: print "(*(1x,at))", s
```

It rewrites `trim(x)` as `x`, and an implied-DO of `trim(s(i))` as `s`, or
`s(a:b)` when the loop covers part of the array, changing each A descriptor
that they meet to AT. The rewrite must be equivalent: every item meeting a
changed descriptor must be one of these or a value without trailing blanks,
such as `"name:"`, so `print "(*(1x,a))", label, trim(name)` is not reported,
and a descriptor with a width, such as `a10`, is not changed. Only output
statements with a literal format are checked, and only rewrites that shorten
the statement are suggested. The check is opt-in, since compilers without
Fortran 2023 support reject AT; with `--std=f2023` it is also in `-Wstyle`.

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

`-Winfinite-loop`, enabled by `-Wall`, warns about a DO loop without loop
control, or `DO WHILE (.true.)`, that nothing in its body can end:

```
warning: this DO loop has no EXIT, RETURN, STOP or branch out of it, so it never ends [infinite-loop]
  = add an EXIT condition, or use `!#allow(infinite-loop)` if the program is meant to run until stopped
```

These end such a loop:
- an EXIT of the loop, including from an IF, SELECT CASE or BLOCK inside it, but
  not an EXIT of a nested loop;
- a named EXIT, GO TO, or I/O with `ERR=`, `END=` or `EOR=`;
- RETURN, STOP or ERROR STOP;
- a call of a procedure that may stop the program, directly or through the
  procedures it calls, or of a procedure outside the program.

Intrinsic procedures such as `RANDOM_NUMBER` never stop it. When control
cannot continue after the loop, for example because its body ends only by
STOP, the statements after it are reported by `-Wunreachable-code`. Disable
with `-Wno-infinite-loop`, or use `!#allow(infinite-loop)` for a program meant
to run until it is stopped. Run `python tests/test_infinite_loop.py`.

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

`-Wrecursive-io` is **on by default**. It warns about an input/output
statement in a procedure that a function referenced by another input/output
statement can reach, directly or through the procedures it calls, when both
use the same external unit. Such recursive input/output is not allowed
(Fortran 2018, 12.12), and gfortran stops with "Recursive I/O not allowed".
`PRINT`, `*` and unit 6 are standard output, and `READ *`, `READ(*,...)` and
unit 5 standard input. Writing to an internal file or to another unit is
allowed and not reported, nor is a function evaluated before the statement,
as in `y = f(x)` followed by `print *, y`. The warning appears once for each
inner statement, with a label at each reference that can reach it. Disable
with `-Wno-recursive-io` or `!#allow(recursive-io)`; promote with
`-Werror=recursive-io`.

`-Wunit-leak` is enabled by `-Wall`. It warns about an
`open(newunit=u, ...)` in a procedure, where `u` is a local INTEGER variable
that is not SAVE (or initialized), a dummy argument or the function result,
and the procedure has no CLOSE of `u`. The file stays connected to a unit
whose number is lost when the procedure returns, and each call connects
another; calling it again with the same FILE= connects one file to two units.
A variable used other than as a unit, for example assigned to a module
variable or passed to a procedure, and procedures with internal procedures,
are not checked. Main programs are exempt, since their units are closed when
the program ends.

`-Wpreconnected-unit` is enabled by `-Wextra`. It warns about READ from unit
5 and WRITE to unit 6 or 0, given as an integer literal or a named constant.
Which units are preconnected, and to what, is processor dependent; `*`, and
INPUT_UNIT, OUTPUT_UNIT and ERROR_UNIT of ISO_FORTRAN_ENV (Fortran 2003), are
portable. The note suggests them, or for a named constant, defining it as the
ISO_FORTRAN_ENV name. A unit the program OPENs is exempt. With `-std=` before
Fortran 2003 the note suggests only `*`, and unit 0, which then has no
portable spelling, is not reported.

`-Wfunction-io` is enabled by `-Wstyle`. It warns about a function that
executes input/output on an external unit, once per function. Such a side
effect keeps the function from being PURE, and referencing it in an
input/output statement on the same unit is recursive input/output. Writing to
an internal file is not reported.

`-Wunhandled-io-error` is opt-in, enabled by neither `-Wall` nor `-Wextra`.
In a function or subroutine, it warns about an OPEN without IOSTAT= or ERR=,
and a READ from a file or an internal file without IOSTAT=, ERR= or END=.
Without one, a missing file, an end of file or invalid data stops the
program, which takes the decision away from the procedure's caller. An
internal READ such as `read (text, *) x`, converting text to a number, is the
common case. END= alone counts as handling a READ. READ from standard input,
output statements, CLOSE, INQUIRE, OPEN of a scratch file, and main programs
are not reported. Regression checks for these five warnings:
`python tests/test_io_warnings.py`.

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

A named constant set to a kind number, as in `integer, parameter :: wp = 8`,
is reported at its declaration when the file uses it as a kind, in a type
specification, a literal suffix such as `1.0_wp`, or a conversion KIND
argument. The suggestion depends on the use: `wp = real64` for REAL or COMPLEX
kinds, `wp = int64` for INTEGER kinds; a constant used for both is advised to
become separate constants. Portable values such as `kind(1.0d0)`,
`selected_real_kind(15, 307)` or `real64` are not reported.

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

## Default REAL declarations

`-Wdefault-real` reports REAL and COMPLEX declarations without a kind:
variables, dummy arguments, function results, derived-type components,
`IMPLICIT REAL (a-h)`, and typed ALLOCATE and constructors.

```
warning: `x`, `y` declared as default REAL [default-real]
2 │ real :: x, y(3)
  │ ^^^^ use real(real32)
```

The suggestion uses the REAL kind constant the file uses most often, such as
`dp` in `real(dp)`; otherwise `real32` from ISO_FORTRAN_ENV, which keeps the
current precision (use `real64` if double precision was intended). Changing a
kind changes results, and literals such as `0.1` need the kind too, which
`-Wreal-literal-kind` finds. INTEGER, LOGICAL and CHARACTER declarations,
`DOUBLE PRECISION`, `REAL*8` (reported by `-Wliteral-kind`) and conversions
such as `real(n)` are not reported. Each declaration statement gets one
warning.

The warning is opt-in, outside both `-Wall` and `-Wextra`. Use
`!#allow(default-real)` for code that needs default REAL, such as interfaces
to single-precision libraries.

Regression checks: `python tests/test_default_real.py`.

## Indentation

`-Windentation` reports free-form layout that hides the structure of IF, DO,
SELECT, WHERE, FORALL, BLOCK, ASSOCIATE, CRITICAL and CHANGE TEAM
constructs, interface blocks, derived type definitions and enumerations:

```
warning: statement is indented inconsistently with the rest of the DO loop [indentation]
  ┌─ main.f90:3:5
  │
2 │    print *, "i", i
  │    --------------- the body starts at column 4
3 │     print *, "i**2", i**2
  │     ^^^^^^^^^^^^^^^^^^^^^ column 5
```

- A body not indented past its opening statement. Statements after a CASE
  are indented past the CASE statement.
- Statements of one body that start in different columns. The branches of an
  IF and the cases of a SELECT count as one body.
- END, ELSE IF, ELSE and ELSE WHERE statements not aligned with the opening
  statement. CASE statements may be aligned with SELECT or indented past it,
  but consistently; CONTAINS in a type may not be indented less than TYPE.

No indentation width is required. Each construct gets at most one body
warning and one alignment warning. Bodies of programs, modules and procedures
are not checked, since many styles do not indent them, but constructs inside
them are. Only the first line of each statement counts, from the keyword after
any label; statements after `;`, statements ending labeled DO loops and
fixed-form source are not checked. Tabs advance to the next multiple of 8
columns. A formatter such as fprettify or findent can reindent a file.

The warning is opt-in, outside both `-Wall` and `-Wextra`.

Regression checks: `python tests/test_indentation.py`.

## Early RETURN

`-Wearly-return` reports a RETURN, conditional or not, after which
executable statements of the procedure can still run:

```
warning: RETURN before the end of subroutine `scale` [early-return]
8 │ if (n <= 0) return
  │             ^^^^^^ later statements of `scale` can still run
```

A RETURN that is the last executable statement is not reported, since that
form is common and harmless. Neither is a RETURN followed only by END IF,
END SELECT, END BLOCK, END ASSOCIATE, CONTINUE, FORMAT and DATA statements
and by other branches of its IF and SELECT constructs, which it never falls
into. A RETURN before an ENTRY statement is needed and is not reported. A
RETURN in a DO loop, including a labeled one, is reported with a note that it
also ends the loop. Code after an unconditional RETURN is also reported by
`-Wunreachable-code`. Fixed-form source is checked.

The warning is opt-in, outside both `-Wall` and `-Wextra`.

Regression checks: `python tests/test_early_return.py`.

## Style warnings

`-Wstyle` enables the checks of rules from Clerman and Spector's *Modern
Fortran: Style and Usage*. Each is opt-in and can also be selected alone, and
`-Wno-<name>` removes one from the group, as in `-Wstyle -Wno-goto`:

- the new style checks `-Wold-relational`, `-Wlogical-comparison`,
  `-Wmissing-double-colon`, `-Wlabeled-do`, `-Wdouble-precision`,
  `-Wbare-end`, `-Wkeyword-case`, `-Wtabs`, `-Wline-length`,
  `-Wsemicolon`, `-Wtrailing-whitespace`, `-Wunnamed-exit`, `-Wgoto`, `-Wobsolescent`, `-Wstop-in-procedure`,
  `-Wassumed-size`, `-Wredundant-implicit`, `-Wname-case`,
  `-Wfunction-result`, `-Wduplicate-constant`, `-Wdeclaration-order`,
  `-Wargument-order` and `-Wdo-while-true`;
- the existing checks `-Windentation`, `-Wuse-without-only`,
  `-Wmodule-default-public`, `-Wliteral-kind`, `-Wdefault-real`,
  `-Wcould-be-parameter`, `-Wmissing-intent` and `-Wimplicit-typing`.

```
warning: `.lt.` is the old spelling of `<` [old-relational]
10 │ if (n .lt. 0) stop
   │       ^^^^ use <
   = suggestion: if (n < 0) stop

warning: EXIT without a construct name in this DO loop [unnamed-exit]
11 │ do i = 1, n
   │ ----------- this loop
12 │    if (x(i) > 0) exit
   │                  ^^^^ `exit` applies to the innermost DO loop
   = name the loop, as in `name: do`, and write `exit name`; a named EXIT or CYCLE keeps its meaning if loops are added around it
```

Notes on individual checks:

- `-Wlogical-comparison` and `-Wcompare-reals` are also in `-Wextra`, since
  they often find mistakes. `-Wcompare-reals` is not part of `-Wstyle`; it
  skips comparisons with an exact zero, such as `if (sigma == 0)`.
- `-Wline-length` uses 132 characters, the free-form limit before Fortran
  2023 and the gfortran default; `-Wline-length=100` sets another limit.
  `-Wtabs`, `-Wline-length` and `-Wtrailing-whitespace` check free-form
  source only, as does the trailing-semicolon part of `-Wsemicolon`.
- `-Wduplicate-constant` compares the files compiled together, and
  compares values after evaluating them at the kind of each constant, so
  `3.141592653589793_dp` and `4*atan(1.0_dp)` are the same value. INTEGER
  constants other than kind parameters, and LOGICAL and CHARACTER constants,
  are not reported, since a shared name such as `n` is usually a
  coincidence.
- `-Wdeclaration-order` checks functions, subroutines and interface bodies.
  A dummy procedure counts as declared at its interface body or PROCEDURE
  statement; INTENT, OPTIONAL and DIMENSION statements do not move a
  declaration. One warning is given per procedure for the order, and one
  for a declaration that mixes groups, with the statement split as a
  suggestion.
- `-Wargument-order` wants required INTENT(IN) arguments first (or VALUE
  ones without INTENT), then the other required arguments, then OPTIONAL
  ones. Dummies without INTENT are not compared by intent. Interface bodies,
  a CLASS passed-object first argument, and the procedures of defined
  assignment and defined I/O, whose order is fixed, are not reported. The
  suggested list keeps each group in its order; positional calls must change
  to match it.
- `-Wkeyword-case` accepts any one case, lowercase, uppercase or
  capitalized, and reports the keywords not in the file's most common case.
- `-Wbare-end` covers programs, modules, submodules, functions,
  subroutines, separate module procedures, block data and derived types.
- `-Wobsolescent` reports plain GO TO under `-Wgoto` instead, since GO TO
  itself is not obsolescent, and labeled DO loops under `-Wlabeled-do`,
  which notes when shared or action-statement termination is obsolescent.
- `-Wredundant-implicit` reports IMPLICIT NONE that changes nothing because
  the host's applies. `implicit none (type, external)` under a host's plain
  `implicit none` adds the EXTERNAL rule and is not reported.
- `-Wname-case` compares each use with the declaration in the scope that
  owns the name: the procedure, its host, or the module it was USE'd from,
  with components compared with their declarations in types. Names that
  are not declared are compared with their first appearance. Keyword
  arguments and I/O specifiers are not checked.
- `-Wfunction-result` prefers `function f(x) result(y)` with `y` declared
  in the body: a prefix type cannot carry attributes such as ALLOCATABLE,
  and RESULT keeps the function's name for calls. Without RESULT, the
  suggestion uses `f_result` and labels the uses of `f` to rename.
  Separate module procedures (`module function`) and assumed-length
  `character(len=*)` functions are not reported.
- `-Wcould-be-conditional` is not part of `-Wstyle`, since conditional
  expressions are new in Fortran 2023. It needs an ELSE branch and at most
  three branches, each a single assignment to the same variable, with
  alternatives of one type and kind; integer and REAL literals are rewritten
  to match REAL alternatives, as in `-1` becoming `-1.0`. Unlike MERGE, a
  conditional expression evaluates only the chosen alternative.
- `-Wcould-be-one-line-if` and `-Wone-line-if` express opposite
  preferences, so neither is in `-Wstyle`. A block is suggested as a one-line
  IF only when it has no ELSE, construct name, labels, comments or `;`, its
  statement fits on one line and can be a logical IF action, and the result
  fits in 132 characters, or the `-Wline-length=N` limit.
- `-Wassumed-size` is advice: an assumed-shape dummy changes how the
  procedure is called and needs an explicit interface.

Regression checks: `python tests/test_style_warnings.py`.

## Array conformance

Arrays combined by an elemental operation must have the same shape, and
operands that do not conform give undefined results. For a procedure whose
array dummies are assumed-shape, nothing checks this:

```fortran
function sumproduct(x, y) result(xy)
real, intent(in) :: x(:), y(:)
real :: xy
xy = sum(x*y)
end function sumproduct
```

`-Wunchecked-conformance`, in `-Wextra`, reports the relationship and the
check that would enforce it:

```
warning: `x*y` requires `size(x) == size(y)`, which `sumproduct` does not check [unchecked-conformance]
  = suggestion: if (size(x) /= size(y)) error stop "sumproduct: x and y do not conform"
```

The relationships come from elemental operators and intrinsics, which need
equal sizes, or equal shapes for arrays of rank 2 or more; `dot_product`,
which needs equal sizes; `matmul`, which needs `size(a, 2) == size(b, 1)`
or its rank-one forms; and assignment to a dummy that is not ALLOCATABLE,
as `y = 2*x`. A procedure counts as checking a relationship when some
comparison combines the sizes of both arrays, as `size(x) /= size(y)` or
`any(shape(a) /= shape(b))`, directly or through a variable assigned a size,
as `n = size(x)` then `size(y) /= n`, or when a call is given both sizes, as
`call assert_equal(size(x), size(y))`. Declaring a dummy with the size it
must have, as `y(size(x))`, makes it explicit-shape, and it is not reported.
OPTIONAL dummies are not considered.

`-Wconformance`, in `-Wall`, reports a call whose actual arguments, of sizes
known at compile time, break such a relationship, whether or not the
procedure checks it:

```
warning: `sumproduct` requires `size(x) == size(y)`, but this call passes 2 and 1 elements [conformance]
13 │ print*,sumproduct([10.0, 20.0], [30.0])
```

The sizes of array constructors of scalars and of arrays with constant
bounds are known; sections and arrays of unknown size are not checked.

Two further checks in `-Wextra` follow arrays whose sizes depend on the
data. Within a procedure or main program, an array defined once, outside any
construct, gets its size from that definition: `x1 = pack(x, tf)` gives 0
to `size(x)` elements, `count(tf)`; `allocate(c(n))` gives `n`; and `x = y`
gives the size of `y`. Arrays with constant bounds have theirs. Assigning a
scalar, as `c = 1`, keeps the size; any other definition, or one inside a
construct, stops the tracking.

```fortran
x1 = pack(x, tf)
x2 = pack(x, .not. tf)
print *, x1 - x2
print *, x1(1), x1(size(x))
```

```
warning: `x1 - x2` combines arrays whose sizes may differ: `x1` has count(tf) elements and `x2` has count(.not. tf) [maybe-nonconforming]
warning: `x1(1)` is out of bounds when `x1` is empty; `x1 = pack(x, tf)` gives 0 to 10 elements [maybe-out-of-bounds]
warning: `x1(size(x))` is `x1(10)`, out of bounds unless all 10 elements of `tf` are true; `x1 = pack(x, tf)` gives 0 to 10 elements [maybe-out-of-bounds]
```

`-Wmaybe-nonconforming` reports an elemental operation on two tracked
arrays unless their sizes are the same: one size expression, as `c(n)` and
`d(n)`, or PACK results of one array with one mask. `-Wmaybe-out-of-bounds`
reports a subscript of a PACK result whose value is known, as `1` or
`size(x)` for `x` of constant size. Neither reports an array whose size, or
whose PACK mask's count, some comparison, DO bound or procedure argument of
the scope uses, as in `if (size(a) > 0)`, `if (count(tf) >= 2)` or
`do i = 1, size(g)`.
Regression checks: `python tests/test_conformance_warnings.py`.

## Constant conditions

`-Wconstant-condition`, in `-Wextra`, reports a condition, or part of one,
whose value is known at compile time, so that part of the test has no
effect:

```
warning: `.true. .or. x > y` is always true; `x > y` has no effect [constant-condition]
```

It checks IF, ELSE IF and DO WHILE conditions, WHERE and MERGE masks,
the conditions of conditional expressions, and LOGICAL assignments such as
`flag = .false. .and. g`. It reports:

- literals combined with other operands, as `.true. .or. e` and
  `.false. .and. e`, which are constant, and `.true. .and. e`, where the
  literal has no effect;
- an INTEGER, LOGICAL or CHARACTER variable compared with itself, as
  `n == n` or `s /= s`. REAL and COMPLEX are not reported, since `x /= x`
  is the test for a NaN;
- comparisons of one INTEGER variable with constants that contradict or
  cover every value, as `n > 5 .and. n < 3` or `n > 5 .or. n <= 5`;
- comparisons of literals, as `1 > 2` or `"ab" == "ab "`.

`if (.true.)` without ELSE is reported as having no effect, and
`else if (.true.)` as an ELSE. Other literals that are the whole condition
are left to other checks: `-Wunreachable-code`, in `-Wall`, reports the
branch that `if (.false.)`, or `if (.true.)` with ELSE, makes unreachable,
and `-Wdo-while-true`, in `-Wstyle`, suggests `do` for `do while (.true.)`. Values that come from named
constants are not reported either, since constants such as
`logical, parameter :: debug = .false.` are usually settings.
`-Wconstant-condition-all`, which is not in any group, reports them too, and
says where the value comes from:

```
warning: `tf .or. x > y` is always true, since `tf` is a named constant with value .true.; `x > y` has no effect [constant-condition-all]
warning: `debug` is a named constant with value .false., so the condition is always false [constant-condition-all]
```

Regression checks: `python tests/test_constant_conditions.py`.

## Uninitialized variables

`-Wmaybe-uninitialized`, in `-Wextra`, reports a local scalar read where some
path from the start of its procedure or main program reaches the read
without assigning it:

```
warning: `x` may be used before it is assigned [maybe-uninitialized]
11 │ if (n > 2) then
12 │    x = 1.0
13 │ end if
14 │ print *, x
   │          ^
```

A variable never assigned anywhere is reported as "used but never
assigned". Each variable is reported once, at its first such read.

The analysis is conservative, so that each report is a real path:

- It considers local scalars of intrinsic type. Dummy arguments, function
  results, named constants, and variables that are initialized, SAVE,
  POINTER, TARGET, ALLOCATABLE, VOLATILE, ASYNCHRONOUS, in COMMON,
  EQUIVALENCE, DATA or NAMELIST, or referenced by a contained procedure are
  not checked.
- A variable passed by name to a procedure, or named in READ, ALLOCATE,
  INQUIRE, OPEN or another I/O statement other than PRINT, counts as
  assigned there, since it may be an output.
- After a DO loop, only what was assigned before the loop counts, since the
  loop may run zero times. After a DO loop without loop control, what was
  assigned on every path to its EXIT statements counts.
- Procedures with GO TO, statement labels or I/O branches (ERR=, END=,
  EOR=) are not analyzed.

The same analysis gives two checks of dummy arguments, also in `-Wextra`.
`-Wintent-out-unset` reports an INTENT(OUT) dummy that some path to the end
of the procedure or to a RETURN does not assign, so that the caller receives
an undefined value; assigning an element or component counts.
`-Wcould-be-intent-out` reports an INTENT(INOUT) scalar that every path
assigns before anything reads it or passes it to a procedure, so that its
value on entry is never used. Both consider dummies of intrinsic type that
are not OPTIONAL, POINTER, ALLOCATABLE, TARGET, VALUE, VOLATILE or
ASYNCHRONOUS, nor referenced by a contained procedure.

Regression checks: `python tests/test_flow_warnings.py`.

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


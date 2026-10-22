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

## Warnings

The following source advice shares the existing `-Wno-<category>`,
`-Werror=<category>`, and `!#allow(category)` / `!#deny(category)` controls.
Diagnostics identify original source locations; shadowing, dead-store, aliasing,
and lifetime diagnostics also identify the related declaration or assignment.

| Option | In `-Wall` | Initial scope |
| --- | --- | --- |
| `-Wshadow` | No | Local data declarations hiding enclosing variables or named constants, including USE-associated names |
| `-Wshadow-dummy-argument` | No | The corresponding check for dummy arguments, separately selectable |
| `-Wdead-store` | No | Straight-line assignments to local scalars overwritten before a read |
| `-Winteger-division` | Yes | Constant integer divisions with a nonzero discarded remainder, including named constants |
| `-Wunchecked-status` | No | Local scalar IOSTAT, allocation STAT, and intrinsic status results not inspected or forwarded |
| `-Wtarget-lifetime` | Yes | Pointer associations escaping a known automatic local target |
| `-Waliasing` | Yes | Known overlapping actuals when a defined procedure's OUT dummy, or modified INOUT dummy, conflicts with another dummy's reads |

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
rfort -Wall -Wshadow -Wshadow-dummy-argument -Wdead-store -Wunchecked-status check example.f90
```

Regression checks: `python tests/test_source_advice.py`.

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

`-Wmissing-intent` is opt-in and is **not enabled by `-Wall`**. It warns about
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
`rfort -Wall -Wmissing-intent check tests/programs/xdeclaration_warnings.f90`.
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

# Standalone repros from the Fortran 2015 samples

## Follow-up: early deferred CHARACTER array rejection fixed

The declaration at `statistics.f90:1689` now passes. Deferred-length CHARACTER
arrays already have runtime storage support, but early constant-folding
snapshots rejected them before that storage lowering ran. The constant pass
now preserves their deferred length, as it already did for deferred scalars.

[tests/programs/xoptional_character_array_copy.f90](../tests/programs/xoptional_character_array_copy.f90)
reproduces the exact diagnostic using a mixed scalar/array declaration,
assignment from optional assumed-length CHARACTER arrays, and unrelated saved
derived-object storage that triggers the early snapshot. It checks both the
present and absent optional argument, including different array sizes and
element lengths. The focused suite is
[tests/test_early_deferred_character_arrays.py](../tests/test_early_deferred_character_arrays.py).
Its second test checks runtime-length ALLOCATE and reallocation on assignment.
That test also exposed and fixes an empty typed-constructor shape bug: an empty
CHARACTER constructor must provide both its declared element length and size
zero, rather than retaining the destination's previous element length.
All 37 tests in the focused suite and the existing deferred CHARACTER array,
deferred CHARACTER local, early intrinsic keyword, empty constructor and
TOKENIZE suites pass. The new regressions compare interpreter and native
`-O0`/`-O2` output with gfortran, including empty constructors with zero and
runtime-dependent nonzero element lengths.
The release compiler was rebuilt, both new regressions pass against that
binary, and `rf.bat tests\programs\xoptional_character_array_copy.f90 --run`
prints the expected label sizes, lengths and contents.

The isolated full-group follow-up at
`target/2015-compat/deferred-character-final-20261103/` gets beyond the
statistics declaration. Neither group builds completely yet:

| Group | Next diagnostic | Reported location |
|---|---|---|
| `fve_files.txt` | constructor implied-DO index `imetric` requires a locally declared scalar INTEGER variable | `vol_pred.f90:103`, enclosing named DO |
| `rcsv_files.txt` | unsupported expression in numeric array reduction | `read_csv.f90:100`, internal READ |

Both groups still compile with gfortran. The follow-up compiles and links only;
it does not execute the external programs. Source copies, compiler snapshot,
commands and full diagnostics are preserved in the report directory.

## Follow-up: utility/statistics PARAMETER errors fixed

The executable assignments at `statistics.f90:1082` and `util.f90:8070` now
pass the earlier PARAMETER diagnostics. Reduction established the common cause:
saved derived-object preparation requests an early constant-folding snapshot,
before ordinary keyword association. The scalar-intrinsic folder interpreted
the keyword wrapper in `real(...,kind=dp)` as an integer expression, instead
of associating KIND with its value first. Runtime data and bounds do not need
to be constant.

[tests/programs/xbin_prob_result.f90](../tests/programs/xbin_prob_result.f90)
preserves the statistics assignment, its two array-result functions, and the
unrelated saved/allocated derived objects needed to trigger the early snapshot.
It reproduced the exact PARAMETER error before the fix. It now prints
`1.0`, followed by bin probabilities `0.2500 0.2500 0.5000`.

The fix associates keywords using the existing intrinsic signatures before
folding the supported scalar conversions and inquiries. Visible user procedures
and generics retain their own keyword names. The five tests in
[tests/test_early_intrinsic_keywords.py](../tests/test_early_intrinsic_keywords.py)
check the original statistics trigger, the utility recursive REAL-conversion
statement, reordered conversion/inquiry/kind keywords, user-defined REAL, and
invalid keyword diagnostics. They pass in the interpreter, native `-O0` and
`-O2`, and gfortran where applicable. The existing named-argument,
initializer/kind and persistent-polymorphic-pointer suites pass too (eight,
two and 13 tests).
The release compiler was rebuilt; the five focused tests pass against that
binary too. `rf.bat tests\programs\xbin_prob_result.f90 --run` produces the
expected probabilities.

Isolated comparisons of `fve_files.txt` and `rcsv_files.txt` both get past the
original errors and now stop at `statistics.f90:1689`:

```fortran
character (len=:), allocatable :: indep_label_,cstats_indep_(:)
```

The new diagnostic is `deferred CHARACTER arrays are not yet supported`.
Both groups still build with gfortran; neither is yet a successful full rfort
build. Snapshots, commands and logs for the follow-up are saved locally in
`target/2015-compat/early-keywords-fixed-20261103/`.

## Follow-up: recursive numeric array results fixed

The recursive REAL array-result repro now compiles and runs. Recursive
explicit-shape INTEGER, REAL, DOUBLE PRECISION and LOGICAL array results use
caller-owned buffers and the existing native recursive array descriptors,
instead of recursively expanding the character/array-function inliner.
ALLOCATABLE, POINTER, CHARACTER and derived-type recursive results are outside
this change.

The tracked regression fixture is
[tests/programs/xrecursive_real_array_result.f90](../tests/programs/xrecursive_real_array_result.f90).
[tests/test_recursive_array_results.py](../tests/test_recursive_array_results.py)
checks direct and mutual recursion, runtime matrix shapes, zero-sized and
LOGICAL results, scalar and array actuals evaluated once, and calls in repeated
loop conditions. Its seven tests pass with the interpreter, native `-O0` and
`-O2`, and gfortran. The existing recursive-array and forward-array-result
suites also pass (14 and eight tests, respectively).
The release compiler was rebuilt, all seven focused tests also pass against
that binary, and `rf.bat tests\programs\xrecursive_real_array_result.f90 --run`
prints `1.0 2.0 3.0`.

A focused driver using the original `kind.f90` and `moving_average.f90` now
builds and runs, including the recursive oversized-window path. For input
`[1,4,9]` and window 5, both rfort and gfortran print
`1.0000 2.5000 4.6667`. Its inputs and results are saved locally under
`target/2015-compat/recursive-results-vector-driver-20261103/`.

The full `mavg_files.txt` group gets past the original recursive-result error
and reaches `moving_average.f90:87`, where a column section inside FORALL is
still rejected. The latest isolated comparison is under
`target/2015-compat/recursive-results-mavg-final-20261103/`.
At that stage, the `fve_files.txt` and `rcsv_files.txt` groups still reported
their original PARAMETER diagnostics at `util.f90:8070` and
`statistics.f90:1082`; recursion support alone did not resolve them. The newer
keyword-folding follow-up above fixes those errors. The earlier logs are under
`target/2015-compat/recursive-results-final-20261103/`.

The table below preserves the original observations against the older binary.

## Original repro verification

Checked on 2026-11-03 using an existing Windows x86-64 rfort
0.1.0-alpha.1 executable, SHA256
`a89f046e71e2f63838bf988d04dabcba2f92bb2103b88df0612f712bbbcc76a0`.
No compiler source was changed or rebuilt. GNU Fortran (GCC) 17.0.0
20260510 (experimental) compiles all six cases below. Its executables were
run for the five standard-language cases and produced the expected output.
The SYSTEM extension case was compiled and linked only.

These cases reduce findings in the
[third sample](FORTRAN_2015_SAMPLE3.md). The automatic-array case preserves a
valid failing construct but changes the diagnostic; it is not an exact
reproduction of the original error message. The other five reproduce the
corresponding diagnostic or unresolved symbol.

| Source | Origin | Observed rfort failure | Expected reference behavior |
|---|---|---|---|
| [xautomatic_array_lower_bound.f90](../xautomatic_array_lower_bound.f90) | `upd_files.txt`, `lsq_miller.f90:560` | `undefined function or subroutine rms` at a valid array declaration | Bounds 2 and 4; values 2, 3, 4 |
| [xdummy_named_end.f90](../xdummy_named_end.f90) | `eph_files.txt`, `m_time.f90:3731` | Rejects `end` as a dummy argument name | Prints 3 |
| [xcomputed_goto_if.f90](../xcomputed_goto_if.f90) | `ucm_files.txt`, `uncon_min.f90:1557` | Rejects `to` in single-line IF containing computed GO TO | Prints 20 |
| [xread_advance_iostat.f90](../xread_advance_iostat.f90) | `ivs_files.txt`, `iso_varying_string.f90` | `READ currently supports one status or branch control` | Reads A, reports IOSTAT 0 |
| [xsystem_extension.f90](../xsystem_extension.f90) | `rgn_files.txt`, `xrgnuplot.f90` | LLD: `undefined symbol: rfort$system` | gfortran accepts the SYSTEM extension |
| [xrecursive_real_array_result.f90](../xrecursive_real_array_result.f90) | `mavg_files.txt`, `moving_average.f90:68` | Calls a REAL result an unsupported recursive CHARACTER function | Prints 1, 2, 3 |

## Automatic-array distinction

The local array has bounds `in+1:ncol`, with `in` an integer dummy argument
and `ncol` a module variable assigned before the call. Both are available on
procedure entry. In this reduction, rfort misidentifies the array declaration
as an undefined procedure reference. In the original larger program it instead
reports `invalid bounds for array rms`, followed by a constant-expression
requirement for `in`.

A control variant with both `in` and `ncol` passed as dummy arguments compiles
with rfort. Thus nonunit lower bounds alone do not reproduce the failure.
Another failing probe uses `rms(ncol)`, suggesting the module-variable bound
matters even without a nonunit lower bound. These probes identify a narrower
area to investigate; they do not establish a single shared underlying cause
for all the original program's bounds handling.

## READ-control distinction

The reduced READ uses `ADVANCE='NO'` together with `IOSTAT=ios`. That combination
alone triggers the diagnostic; no END, EOR or ERR branch control is needed.
The repro creates a formatted scratch file and supplies its own input, so
running the reference program does not require keyboard input or a fixture.

## SYSTEM extension

`CALL SYSTEM` is an extension, not a standard Fortran intrinsic. Its rejection
does not establish a problem with standard `EXECUTE_COMMAND_LINE`. The repro
replaces the original external gnuplot command with harmless `echo system repro`.
The outer rfort message recommends installing Windows build tools, but the
concrete linker diagnostic is an undefined symbol.

## Reproduce and saved evidence

From Windows CMD, for example:

```cmd
target\release\rfort.exe xread_advance_iostat.f90
gfortran xread_advance_iostat.f90 -o target\read_advance_reference.exe
target\read_advance_reference.exe
```

Substitute any of the other source names to check it. Commands use the existing
release binary directly and do not rebuild the compiler.

The checked binary snapshot, full compiler diagnostics, commands and reference
execution results are saved locally under
`target/2015-compat/standalone-repros-20261103/`, including `report.json`.
Exploratory bounds variants are under its `bounds-probes/` subdirectory.
Those generated artifacts are ignored by Git; the six source files and this
document preserve the repros in the repository.

The earlier verified gnuplot FORALL/constructor repro is
[xforall_constructor_implied_do.f90](../xforall_constructor_implied_do.f90).

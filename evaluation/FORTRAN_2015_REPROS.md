# Standalone repros from the Fortran 2015 samples

## fve separate compilation: automatic local array and recursive array result

[mfve_fold_bounds.f90](../tests/programs/mfve_fold_bounds.f90) reduces the
`util.f90:6500` failure to a small module. Its `fold_sizes` subroutine has a
runtime-sized output dummy and an automatic local array. Assigning the result
of recursive `group_sizes` to that local array previously failed when compiling
the module separately:

```text
error: reduction extents and section bounds must be constant integers:
`nfolds` is not a previously defined integer PARAMETER
10 | nelem = group_sizes(n,nfolds)
```

[xfve_fold_bounds.f90](../tests/programs/xfve_fold_bounds.f90) is a small driver
that checks the expected group sizes `4 3 3`. gfortran successfully compiles the
module and driver separately, links them, and runs the check. rfort now does
the same after the native-entry automatic-array fix.

Procedures with array dummies initially skip automatic-local allocation because
their locals are normally allocated during call-site specialization. Recursive
array lowering can instead convert such a procedure to a native entry (including
a nonrecursive bridge that calls a recursive procedure). Those entries now run
the automatic-array allocation step before their calls are rewritten. Their
local arrays retain runtime bounds and have storage per invocation.

Validation: 23 recursive-array tests, 14 separate-module tests, and 5 automatic
array tests pass. Added checks cover the repro with interpreter/native execution,
separate compilation at O0/O2 after deleting the module source, nondefault bounds,
zero extents, and local storage retained across recursive calls.

The rebuilt release compiles the reduced module in 0.121 seconds and its driver
runs with output `4 3 3`. After separately compiling `kind.f90`, the original
`C:/fortran/2015/util.f90` now compiles successfully in 5.129 seconds. These are
single wall-clock measurements; commands and results are saved in
`target/fve-fold-bounds-fixed/release/report.json`. The full combined `fve`
performance bottleneck has not been retested as part of this fix.

CMD commands from the repository root:

```cmd
if not exist target\fve-fold-bounds-repro mkdir target\fve-fold-bounds-repro
target\release\rfort.exe -c tests\programs\mfve_fold_bounds.f90 -J target\fve-fold-bounds-repro -o target\fve-fold-bounds-repro\module.obj
target\release\rfort.exe tests\programs\xfve_fold_bounds.f90 target\fve-fold-bounds-repro\module.obj -I target\fve-fold-bounds-repro -o target\fve-fold-bounds-repro\rfort-driver.exe --run
gfortran -std=f2018 -fcheck=all -c tests\programs\mfve_fold_bounds.f90 -J target\fve-fold-bounds-repro -o target\fve-fold-bounds-repro\module.o
gfortran -std=f2018 -fcheck=all -c tests\programs\xfve_fold_bounds.f90 -I target\fve-fold-bounds-repro -o target\fve-fold-bounds-repro\main.o
gfortran target\fve-fold-bounds-repro\module.o target\fve-fold-bounds-repro\main.o -o target\fve-fold-bounds-repro\driver.exe
target\fve-fold-bounds-repro\driver.exe
```

Before the fix, reduction experiments showed that assigning `group_sizes` directly to the output dummy
compiles; replacing it with a simple nonrecursive array-valued function also
compiles. The failing repro retains the recursive result, automatic local,
and runtime-sized output dummy together. This identifies a separate-compilation
failure; it does not isolate the full group's character-output performance
bottleneck. Verification logs are saved under
`target/fve-fold-bounds-repro/verified/report.json`.

## Character-output compilation-speed follow-up

`output::prepare_character_views` and `output::prepare_formatted` now share
unchanged symbol tables between nested scopes. Generated branches and loops
without declarations no longer copy the entire surrounding table. Adding a
declaration, shadowing a name, or introducing a temporary still creates an
isolated copy. Descriptor metadata is collected separately so an empty update
does not trigger a copy. Both walks also skip subtrees without PRINT or supported
WRITE operations: those generated allocation/bounds-check scopes cannot be
changed by these passes and do not need symbol environments at all.

Opt-in pass timings now distinguish `output.character_views` from
`output.character_views.formatted` within the previously combined pass.

All 88 targeted tests pass: 61 output tests plus 27 formatted implied-DO,
internal array WRITE, runtime-format, and character-array tests. The new
`tests/test_output_scope_sharing.py` covers sibling and shadowed scopes,
runtime output units, and runtime-sized character views in interpreter/native
O0/O2 execution with gfortran comparisons.

The sharing-only attempt still timed out in the character-view walk; pruning
scopes with no output was necessary. In the subsequent uncontended optimized
development build, the formerly blocking pass completes in 2.410 seconds:
1.001 seconds for `output.character_views` and 1.378 seconds for its formatted
output follow-up. This development build uses LTO disabled and 16 codegen units;
do not interpret these measurements as a controlled end-to-end speedup over
the prior normal-release binaries. The build proceeds to `constructors::resolve`.
The compiler/source snapshots, commands, logs and report are saved under
`target/2015-compat/fve-output-pruned/`.

The full run still times out after 300.070 seconds, now in
`constructors::resolve`; gfortran completes in 7.611 seconds. No complete rfort
executable was produced or executed. Constructor lowering is the next hotspot.
The three new scope tests also pass against the rebuilt optimized executable.
`target/release/rfort.exe` currently contains this LTO-disabled development
build; a normal `cargo build --release --bin rfort` restores the repository's
usual release-profile settings.

## Constructor-resolution compilation-speed follow-up

`constructors::resolve` now shares unchanged array-shape tables across nested
scopes and skips scopes without constructor, implied-DO, or SIZE work. SIZE
inquiries outside constructors still undergo resolution, and retained implied-DOs
still receive the existing diagnostics. Local declarations and nonempty descriptor
metadata updates isolate their scope using copy-on-write.

All 119 constructor regression tests pass, including two new cases in
`tests/test_constructor_scope_sharing.py` covering shadowed arrays, sibling scopes,
constructor snapshots, and SIZE inquiries in nested control flow. These exercise
interpreter and native O0/O2 execution with gfortran comparisons.

Using the same optimized development build settings as the preceding output
profile (LTO disabled, 16 codegen units), the full fve group now completes
`constructors::resolve` in **0.149 seconds**. The preceding run was still in this
pass at the overall 300-second limit. Character-output preparation takes 2.442
seconds, and subsequent character-array lowering also completes.

The full build still times out after 300.083 seconds, beyond character-array
lowering; gfortran completes in 9.455 seconds. No complete rfort executable was
produced or run. Compiler/source snapshots, commands and logs are saved under
`target/2015-compat/fve-constructor-pruned/`. This demonstrates removal of the
constructor hotspot, not completion of the overall compilation-speed work.

Opt-in pass timers now also distinguish the subsequent substring, function-array,
and multidimensional-array passes so the remaining work can be located precisely.

The final instrumented build also passes both new scope tests. Its diagnostic
follow-up (`target/2015-compat/fve-post-constructor-profile/`) completes constructor
resolution in 0.140 seconds and reaches the 180-second overall limit inside
`substrings::resolve`. Substring lowering is the next confirmed hotspot.
`target/release/rfort.exe` remains the LTO-disabled optimized development build.

## Substring-lowering compilation-speed follow-up

Substring lowering now skips generated scopes with no character work and shares
unchanged numeric symbol/array-shape tables. Capacity inference no longer scans
nested bodies when there are no local deferred-character targets. Module-binding
keys are retained directly instead of cloning the entire statement tree.
Character declarations, substring operations, intrinsic diagnostics, procedure
entry checks and SELECT CASE remain on the lowering path.

All 42 targeted tests pass (scalar and parameter substrings, deferred-character
locals/module storage, runtime character storage, character intrinsics, and the
new `tests/test_substring_scope_sharing.py`). The new regression checks shadowed
character/numeric variables and overlapping runtime substring assignments in
interpreter/native O0/O2 execution against gfortran.

The initial sharing/tree-copy changes alone still timed out inside the dynamic
substring walk at the overall 300-second limit
(`target/2015-compat/fve-substring-sharing/`). Skipping irrelevant scopes was also
needed. With the final change, substring lowering completes in **29.826 seconds**:
29.653 seconds for the dynamic phase and 0.164 seconds for fixed substrings.
Function-array lowering then completes in 23.181 seconds. These are pass times,
not complete build times. Compiler settings remain LTO disabled and 16 codegen
units, as in the preceding profiles.

Snapshots, commands and pass logs for the final run are saved under
`target/2015-compat/fve-substring-pruned/`. Opt-in timing now distinguishes the
dynamic and fixed substring phases.

The final run still times out after 300.079 seconds, now in
`reductions::resolve`; gfortran completes in 8.433 seconds. Multidimensional
lowering completes in 92.126 seconds and is the largest completed individual
pass in this run. No full rfort executable was produced or executed. The new
scope regression also passes against the rebuilt optimized binary. The release
path still contains the LTO-disabled development build used for these profiles.

## Reduction and multidimensional lowering compilation-speed follow-up

Both passes now use copy-on-write scope tables (`src/shared_map.rs`).
Multidimensional lowering shares unchanged shapes and scalar types while local
updates remain isolated. Reduction lowering shares variables, elemental and
scalar-function signatures, character-function signatures, and procedure names.
Its implicit READ-destination discovery now walks immutable child bodies directly
instead of cloning complete statement subtrees at every nesting level.

All 90 targeted regression tests pass, covering fixed/runtime DIM reductions,
logical and masked reductions, section assignments, multidimensional arguments,
WHERE, FORALL, and list-directed READ. The new
`tests/test_reduction_multidim_scope_sharing.py` checks nested implicit READ
variables and restoration of shapes/types after shadowing blocks, against
interpreter/native O0/O2 execution and gfortran. Both new tests also pass against
the rebuilt optimized executable.

The first attempt, sharing variable/shape tables only, completed multidimensional
lowering in 20.835 seconds but still timed out inside reduction lowering at the
300-second overall limit (`target/2015-compat/fve-reduction-multidim-sharing/`).
Sharing the remaining procedure metadata then allowed reduction lowering to finish.
The final profile (`target/2015-compat/fve-reduction-multidim-final/`) records:

| Pass | Seconds |
|---|---:|
| Multidimensional lowering | 19.764 |
| Reduction lowering | 66.327 |
| Section lowering | 30.302 |

The earlier multidimensional pass took 92.126 seconds. Reduction lowering had no
completed baseline time because it was interrupted at the overall limit. Some
unchanged passes were slower in this run, so these results do not establish a
controlled total-build speedup ratio. Build settings remain LTO disabled and
16 codegen units, as in the previous profiles.

The full compilation still times out after 300.058 seconds, now after starting
semantic analysis; gfortran completes in 7.955 seconds. Semantic analysis has not
yet been measured to completion and is not established as the next bottleneck.
No complete rfort executable was produced or executed. The profiles retain
compiler/source snapshots, commands and full logs. Opt-in reduction counters
separate symbol setup from implicit READ discovery; the latter used roughly
0.32 seconds across the first 98,000 scopes, while symbol setup still accumulated
about 25 seconds. Further declaration-scope optimization remains worthwhile.

`target/release/rfort.exe` contains the LTO-disabled development build used here.

## Separate main-program profile after lowering optimizations

A fresh separate-compilation run used the current optimized compiler snapshot,
compiled all sources in `C:/fortran/2015/fve_files.txt` in list order, and supplied
the resulting module files through `-I`/`-J` when compiling subsequent files.
Each compiler had an isolated workspace. This was compilation only (`-c`),
without linking or execution. Per-file timeout was 600 seconds. rfort used its
default O2 and the LTO-disabled development compiler build; gfortran used O0 and
`-ffree-line-length-none`, so this is not an equal-optimization benchmark.

| Measurement | Result |
|---|---:|
| rfort, all 12 module files | Passed; 34.476 seconds total |
| rfort, main `xfwd_vol_ema_pred.f90` | Timed out after 600.051 seconds |
| gfortran, main `xfwd_vol_ema_pred.f90` | Passed in 0.533 seconds |

All gfortran module compilations also passed. rfort's main-program compile
finished array-call lowering in 220.009 seconds, reductions in 83.611 seconds,
and sections in 46.011 seconds. These are sequential top-level pass times;
array-call lowering already includes the vector, allocation, substring,
function-array and multidimensional subpasses, so those must not be added again.
Completed top-level timers sum to 370.232 seconds. The last active pass was
`sema`, leaving roughly 230 seconds of the run in semantic analysis (an
approximation because startup and untimed work also contribute to elapsed time).
No semantic diagnostic was emitted before timeout.

Thus separately compiling the modules still leaves a very slow main-program
compilation. Semantic analysis is now a confirmed later hotspot and should be
profiled internally next. There is no completed combined-build timing for the
same binary with which to claim that both approaches take equal total time.

The snapshot, per-file commands/timings, source manifests, module/object files,
and pass logs are saved locally under
`target/2015-compat/fve-separate-after-lowering/` (`report.json`, `report.md`, and
`rfort/xfwd_vol_ema_pred.compile.log`). The driver used for this run is
`target/profile_fve_separate_current.py`. No compiler source was changed for
this measurement.

## Internal semantic-analysis profile

Opt-in `RFORT_TIME_PASSES=1` timers now divide semantic analysis into stages.
The instrumented main-only run reused the separately compiled modules from
`fve-separate-after-lowering` and ran with a 600-second timeout. It changed no
semantic rules. The snapshot, command, module origin, report and log are under
`target/2015-compat/fve-separate-sema-profile/`; the local driver is
`target/profile_fve_sema.py`.

| Semantic stage | Seconds |
|---|---:|
| Declarations | 0.009 |
| Early targets and loop checks | 0.046 |
| Initial procedure tables | 0.010 |
| Program reads | 0.123 |
| Source spans and interface collection | 41.816 |
| Reads before assignment | 0.128 |
| Procedure bodies and subsequent top-level checks | 130.850 |
| Undefined calls | 29.166 |
| Unused functions | 0.091 |
| Math checks | Unfinished at overall timeout |

The timer named `sema.function_bodies` includes procedure-body analysis plus
subsequent top-level declaration/assignment checks; it does not establish that
procedure bodies alone take 130.850 seconds. This is the largest completed stage
and the first target for finer profiling and optimization. Source-span processing
and undefined-call checks are also material. Inspection finds repeated token
searches and copied scope tables, but their individual contributions have not
been isolated by these stage timers.

The overall timeout was 600.119 seconds. Completed top-level pre-semantic timers
sum to 342.522 seconds; completed semantic substage timers sum to 202.239 seconds.
The remaining roughly 55 seconds includes unfinished math checks and untimed
startup/work, so it is not an exact math-pass time. No full executable was
produced. Two scope/input smoke tests pass against the instrumented release
binary. Ordinary output remains unchanged unless pass timing is enabled.
The optimized compiler still uses LTO disabled and 16 codegen units.

## Compilation-speed follow-up: fve

The initial release profile timed out after 180 seconds in the final
`vector_subscripts::finish` pass. A second, more detailed profile reached the
300-second limit inside its runtime-vector portion. Each generated branch and
loop eagerly copied the enclosing symbol/shape table, including scopes that
only read existing scalar variables. Large specialized procedures therefore
repeatedly copied thousands of unrelated declarations.

Runtime vector lowering, fixed vector lowering, and allocatable finishing now
share unchanged symbol tables. A scope copies its table only when it introduces
or shadows a declaration, adds a temporary, or changes descriptor metadata.
Empty metadata updates do not force a copy. The mutable-map APIs used by
procedure specialization retain their previous ownership and error behavior.

The vector-only optimized development build completed the final vector pass in
26.834 seconds (runtime: 16.286, fixed: 10.500), then reached the next bottleneck
in `allocatables::finish`. The same scope-sharing optimization was applied to
that pass as well. These intermediate measurements identify the bottlenecks;
they were collected with background builds/tests and are not controlled timing
ratios. The development builds disabled LTO and used 16 codegen units; the final
build uses the repository's normal release profile. The final normal-release result is recorded below.

[tests/test_vector_scope_sharing.py](../tests/test_vector_scope_sharing.py)
checks nested BLOCK declarations, sibling scopes, shadowed scalar/array names,
runtime descriptors, and generated temporaries. Existing vector, pointer,
allocation, character-result and runtime-DIM suites exercise the shared paths.
All 115 tests pass against the combined changes: scope sharing (2), fixed
vectors (8), vector ranks (18), runtime sections (20), pointer elementals (9),
runtime allocatables (19), runtime-DIM reductions (16), allocatable reallocation
(12), and CHARACTER allocatable results (11). Valid cases run in the interpreter
and native `-O0`/`-O2`, with gfortran reference comparisons.

Profiling is opt-in; ordinary compiler output is unchanged. To rerun the
ordered-source compilation from a CMD prompt (use a new output directory):

```cmd
set RFORT_TIME_PASSES=1
python tests\run_source_list_compat.py --root C:\fortran\2015 --compiler target\release\rfort.exe --lists fve_files.txt --limit 1 --compile-timeout 300 --output target\2015-compat\fve-speed-rerun
set RFORT_TIME_PASSES=
```

The runner snapshots the compiler and ordered sources, saves phase timings in
`fve_files/rfort/stderr.txt`, and writes commands/statuses to `report.json` and
`report.md`. It compiles and links only; it does not execute the full program.
Intermediate snapshots and reports are in:

- `target/2015-compat/fve-speed-before-20261103/` (original normal release).
- `target/2015-compat/fve-speed-instrumented-20261103/` (detailed baseline).
- `target/2015-compat/fve-speed-shared-20261103/` (vector-only optimization).
- `target/2015-compat/fve-speed-final-20261103/` (combined changes, normal release).

Final normal-release profile (no concurrent builds or tests):

| Phase | Seconds |
|---|---:|
| Final runtime-vector lowering | 8.085 |
| Final fixed-vector lowering | 3.159 |
| Final vector pass, total | 11.258 |
| Allocatable finishing | 45.014 |
| Character-output preparation | Unfinished at the overall 300-second limit |

The full group still times out (300.057 seconds), now in
`output::prepare_character_views`; it does not yet produce an executable.
Gfortran compiles the same copied group in 9.454 seconds. The old 180-second
limit and the new 300-second limit are **timeout caps**, not completed-build
timings; no end-to-end speedup ratio is established. The phase profile shows
that both previously blocking passes now finish, and identifies character
output preparation as the next bottleneck. That function includes a scope walk
and a subsequent `prepare_formatted` call; finer profiling should distinguish
those before the next optimization.

The release was rebuilt with the normal Cargo profile. Both new scope tests
also pass with that binary, and `rf.bat tests\programs\xfve_pointer_metrics.f90
--run` still produces the five expected metrics. Final compiler SHA256:
`4623305af8d594c80b1c44669ba53c8059e1ae1b61763396d94ede599d313fb5`.


## Follow-up: ELEMENTAL functions of POINTER array expressions

[tests/programs/xfve_pointer_metrics.f90](../tests/programs/xfve_pointer_metrics.f90)
reduces the next `vol_pred.f90:127` failure. POINTERs select columns of allocated
TARGET matrices. The metrics constructor combines RMS/logarithms, ordinary
and logarithmic correlations, and means of the user-defined ELEMENTAL
functions `qlike` and `mse_var` applied to POINTER array expressions. The old
release compiler reproduced `whole POINTER array in this expression is not
yet supported` on this fixture.

The pointer-array lowerer previously retained only function names. It could
apply elemental intrinsics to descriptor elements, but treated a remaining
user-defined ELEMENTAL call as a scalar expression and rejected the whole
POINTER arguments. It now retains ELEMENTAL identity and declared result
types, and emits the scalar function call in its existing descriptor loop.
The result type can differ from the argument type. Scalar actual expressions
are captured once, including an impure scalar function used as an argument.

The new regression suite is
[tests/test_pointer_elemental_functions.py](../tests/test_pointer_elemental_functions.py).
It checks the five original metrics, INTEGER kinds 1/2/4/8, REAL, DOUBLE
PRECISION and LOGICAL, differing result kinds, ranks 2/3/15, reversed and
strided views, nested calls, reassociation, empty arrays, impure ELEMENTAL
invocation counts and scalar evaluation counts. Runtime shape, association
and initialization checks remain tested. Valid outputs are compared across
the interpreter, native `-O0`/`-O2` and gfortran.
All nine new tests pass. The existing pointer-expression-argument, descriptor,
association, allocatable-target and runtime-elemental-reduction suites also
pass (57 tests). The descriptor suite's 10,000-element interpreter case hit
its 15-second limit during concurrent testing and passed when rerun separately.
The release compiler was rebuilt; all nine new tests pass against that binary
too. `rf.bat tests\programs\xfve_pointer_metrics.f90 --run` prints
`0.565952 0.944911 0.866025 0.424196 0.003750`, matching gfortran.

The full rebuilt-release `fve_files.txt` attempt reaches its 180-second compile
limit without a diagnostic. Gfortran builds it in 11.035 seconds. Thus the
standalone metrics failure is fixed, but successful compilation of the full
program has not been established; compilation time is the next investigation.
The compiler snapshot, ordered source copies, commands and logs are at
`target/2015-compat/fve-pointer-elementals-fixed-20261103/`.

## Follow-up: fve constructor index scope fixed

[tests/programs/xfve_constructor_implied_do.f90](../tests/programs/xfve_constructor_implied_do.f90)
now compiles and prints `2.0 3.0 4.0`, matching gfortran. During an early
constant-folding snapshot, the parser's named-loop wrapper is still a BLOCK.
Constructor validation looked only for declarations inside that wrapper and
missed the enclosing procedure's INTEGER declaration of `imetric`.

The validator now includes visible scalar INTEGER type metadata, after USE
association and local shadowing. Local REAL, array and PARAMETER declarations
still hide an enclosing INTEGER and cannot serve as constructor indices.
The seven tests in
[tests/test_early_constructor_scope.py](../tests/test_early_constructor_scope.py)
cover the original repro, nested BLOCK visibility, module host association,
runtime bounds, local shadowing, invalid or missing indices, and preservation
of the enclosing variable's value and initialization state. They pass in the
interpreter and native `-O0`/`-O2`, with gfortran comparisons for valid programs.
The existing array-constructor, constructor-expression, named-loop, early
intrinsic-keyword and early deferred-CHARACTER suites pass too (30 tests).
The release compiler was rebuilt. All seven new tests also pass against that
binary, and `rf.bat tests\programs\xfve_constructor_implied_do.f90 --run`
prints the expected three means.

The full debug-compiler `fve_files.txt` follow-up reached its 120-second compile
limit without a diagnostic; this does not establish that the entire program
builds. Gfortran still builds it. Commands, source copies and logs are at
`target/2015-compat/fve-constructor-scope-fixed-20261103/`.
A separate 90-second pass-timing probe at
`target/2015-compat/fve-constructor-scope-profile-20261103/` reached
`derived::resolve` after parsing and preparation, without completing that pass
before the timeout. This identifies the broad area for further investigation,
not a specific cause of the slowdown.

The rebuilt release compiler does finish far enough to report the next error:
`whole POINTER array in this expression is not yet supported`, at
`vol_pred.f90:127`, the metrics constructor containing expressions involving
`vol_hist_stk` and `vol_fwd_stk`. Its compile attempt took 73.359 seconds;
gfortran built the group in 9.584 seconds. The full group remains blocked.
The release snapshot, ordered sources and logs are at
`target/2015-compat/fve-constructor-scope-release-20261103/`.

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

The standalone `fve` repro is
[tests/programs/xfve_constructor_implied_do.f90](../tests/programs/xfve_constructor_implied_do.f90).
Run `rf.bat tests\programs\xfve_constructor_implied_do.f90 --run`.
Before the scope fix above, both debug and release compilers rejected the declared INTEGER index
`imetric`, pointing to the enclosing named DO. Saved/allocated derived-object
storage is necessary to trigger the early constant-folding pass in this
reduction; without it the same implied-DO compiles. Gfortran with
`-std=f2018 -fcheck=all` compiles and runs the repro, printing `2.0 3.0 4.0`.

Both groups still compile with gfortran. The follow-up compiles and links only;
it does not execute the external programs; the standalone repro was executed
with gfortran. Source copies, compiler snapshot,
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

## Small fve drivers

[Small-driver results and commands](FVE_SMALL_DRIVERS.md) isolate the forecast
statistics-reporting calls: the reduced caller times out at 180 seconds, whereas
a matched module with only those two calls removed compiles in 34.230 seconds.

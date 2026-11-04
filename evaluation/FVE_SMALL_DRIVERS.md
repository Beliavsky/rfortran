# Small drivers isolating the fve compilation slowdown

These compile-only probes reduce `C:/fortran/2015/xfwd_vol_ema_pred.f90`.
They use the original separately compiled modules; they are small main-program
repros, not standalone replacements for the library. No external source or
compiler implementation was changed in this investigation. No driver was run.

## Results

The checked compiler is the instrumented LTO-disabled optimized development
build. rfort uses default O2; gfortran uses O0. Module preparation is excluded
from driver times, and wall times vary between runs.

| Driver | rfort | gfortran |
|---|---|---|
| `xfve_compile_imports.f90` | Compiled, 19.916 s | Compiled, 0.225 s |
| `xfve_compile_read_time_series.f90` | Compiled, 26.354 s | Compiled, 0.359 s |
| `xfve_compile_print_stats.f90` | Failed after 114.328 s | Compiled, 0.249 s |
| `xfve_compile_fwd_vol.f90` | Timed out at 180.190 s in source-span/interface semantic checks | Compiled, 0.255 s |
| `xfve_compile_fwd_vol_no_stats.f90` | Compiled, 34.230 s | Compiled, 0.235 s |

The forecast repro consists of one call to `fwd_vol_ema_pred` with runtime-sized
allocated arrays, initialized synthetic contents, and scalar input controls.
Runtime-sized option arrays matter: an earlier version with fixed one-element
option arrays hit an early `array @sub_local_69 is used before ALLOCATE` error.
That exploratory version is preserved in the first run's source snapshots.

The no-statistics control calls the same forecast routine in
`mfve_compile_no_stats.f90`. That module is copied from the original `vol_pred.f90`
with only its module name changed and the two calls to `print_stats` inside
`fwd_vol_ema_pred` removed. It retains other procedures, calculations, loops,
allocations, reductions, pointer associations, and WRITE statements. The two
main drivers otherwise match. Semantic analysis of this control takes 1.980 s.

This isolates the two statistics-reporting call paths as a major source of
compilation cost. It does not prove that the cost is intrinsic to PRINT or that
any single statement inside the statistics library is responsible. The next
step is to reduce `print_stats_matrix` and its callees, comparing generated code
size and semantic-analysis time.

The direct `print_stats` repro reaches an initialization diagnostic in
`util.f90:3934`, `do i=size(tf),1,-1`, naming generated storage
`@alloc_block_21201` in the final runtime-option run (generated names can change
between variants). It is a separate correctness blocker as well as a much smaller
profiling workload. Do not label its result a successful rfort compilation.

## Run the probes

Sources are in `tests/programs/`. The runner snapshots the compiler, creates
isolated per-driver workspaces, writes live `compile.log` files and a JSON report,
and never executes the generated programs. It requires existing compiler-specific
module directories. The directories below were created by the earlier separate
fve build; they are local ignored artifacts, not repository fixtures.

```cmd
python tests\run_fve_compile_probes.py --modules target\2015-compat\fve-separate-after-lowering\rfort --reference-modules target\2015-compat\fve-separate-after-lowering\gfortran --timeout 180 --output target\2015-compat\fve-probes-new
```

The default sweep runs all standalone `xfve_compile_*.f90` probes except the
no-statistics control that requires its extra module. To run just the strongest
slow repro, append `tests\programs\xfve_compile_fwd_vol.f90`.

The matched control requires its extra module:

```cmd
python tests\run_fve_compile_probes.py --modules target\2015-compat\fve-separate-after-lowering\rfort --reference-modules target\2015-compat\fve-separate-after-lowering\gfortran --extra-module tests\programs\mfve_compile_no_stats.f90 --timeout 180 --output target\2015-compat\fve-control-new tests\programs\xfve_compile_fwd_vol_no_stats.f90
```

Choose a new output directory on each run. Omit `--reference-modules` to skip
gfortran, or use `--compiler` to choose another rfort executable. The runner's
`passed` status means object compilation succeeded, not linking or runtime
validation. Inspect `report.json` for timeouts (a failed compilation gives the
runner a nonzero exit status; timeouts are reported separately).

Saved reports and compiler/source snapshots:

- `target/2015-compat/fve-small-drivers/`: imports, input, and initial fixed-option probes.
- `target/2015-compat/fve-small-runtime-drivers/`: final forecast/statistics repros.
- `target/2015-compat/fve-no-stats-matched-control/`: final full-module control.

An earlier trimmed-module control is under `fve-no-stats-control`; the matched
full-module control above avoids attributing a speedup to removal of unused
procedures.

## Narrowing the statistics dispatcher

A second reduction removes report formatting and calls the generic `stats`
function directly. Its scalar-name implementation (`stat_vec_str`) selects among
many statistics and their callees. Even `stats("mean",x)` takes a materially larger
lowering/semantic path than a direct `mean(x)` call in these measurements.

| New probe | rfort seconds | rfort result | gfortran seconds |
|---|---:|---|---:|
| `xfve_compile_stat_mean.f90` | 5.717 | Compiled | 0.211 |
| `xfve_compile_stat_literal.f90` | 19.094 | EXIT semantic error | 0.200 |
| `xfve_compile_stat_dynamic.f90` | 15.039 | EXIT semantic error | 0.344 |
| `xfve_compile_stats_vector.f90` | 17.535 | Initialization error | 0.279 |
| `xfve_compile_stats_repeat1.f90` | 48.751 | Initialization error | 0.334 |
| `xfve_compile_stats_repeat2.f90` | 68.507 | Initialization error | 0.269 |
| `xfve_compile_stats_repeat4.f90` | >120.058 | Timeout in reduction lowering | 0.489 |

All gfortran object compilations passed. These are compile-only cases; no runtime
correctness comparison was performed. Compiler implementation and external module
files were unchanged. All probes reuse the same existing module files.

The repeat1/2/4 programs are matched: each executes one, two, or four source-level
copies of `y=stats(names,x)` followed by `print *,sum(y)`. All input arrays are
initialized, and every result is used. Their main programs contain only allocation,
synthetic initialization, and repeated calls. The earlier `stats_vector` prints
`y` directly, so its 17.535-second measurement must not be used as the single-call
baseline for the SUM-printing repeat probes. Machine timing variability and
incomplete compilations also preclude claiming a precise asymptotic growth rate.

For the matched one-call probe, semantic analysis takes 20.850 seconds; for two
calls it takes 38.393 seconds. Math checks account for 8.484 and 23.289 seconds,
respectively. The four-call probe spends 91.373 seconds in array-call lowering
before reaching reduction lowering and the timeout. This supports investigating
repeated expansion and the cost of traversing the resulting scopes, rather than
attributing the slowdown solely to formatted output or forecast calculations.

The literal and dynamic scalar-name probes both produce two `EXIT used outside
of a loop` diagnostics during semantic analysis. The multiple-name variants
instead reach an uninitialized generated-storage diagnostic after semantic
analysis. These are smaller correctness repros as well as timing probes; failure
before code generation means their times are not successful-build timings.
Reported source locations can refer into imported/expanded module source, so the
original full diagnostic and module snapshot should accompany a bug investigation.

Recommended next steps:

1. Use `xfve_compile_stats_repeat1.f90` for faster semantic profiling, especially
   math-check scope handling, and repeat2/4 to check scaling after a change.
2. Reduce the scalar `stats("mean",x)` EXIT diagnostic independently.
3. Investigate whether the constant statistic selector can avoid lowering unused
   dispatcher branches, without removing diagnostics required by Fortran.

Example using the existing runner and module snapshots:

```cmd
python tests\run_fve_compile_probes.py --modules target\2015-compat\fve-separate-after-lowering\rfort --reference-modules target\2015-compat\fve-separate-after-lowering\gfortran --timeout 120 --output target\2015-compat\fve-repeat-new tests\programs\xfve_compile_stats_repeat1.f90 tests\programs\xfve_compile_stats_repeat2.f90 tests\programs\xfve_compile_stats_repeat4.f90
```

Reports and per-case source/compiler snapshots are under
`target/2015-compat/fve-stats-dispatch-probes/`, `fve-stats-repeat-probes/`, and
`fve-stats-repeat1-control/`. None of these measurements establishes that the
full statistics or forecasting library runs correctly with rfort.


## Scoped semantic math checks

The math-check traversal now borrows one symbol/array environment, records only
local declaration changes, and restores them when leaving a scope. Previously it
cloned both full tables for every nested body, including declaration-free loops
and conditionals. Reverse restoration preserves shadowing and sibling isolation.
No intrinsic checks or expression traversal paths were removed.

The same optimized development build settings were used (release profile with LTO
disabled and 16 codegen units), with the existing separate module snapshots.
Measurements are sequential, single runs, not statistically controlled benchmarks.

| Matched stats calls | Earlier math checks (s) | New math checks (s) | New total attempt (s) |
|---|---:|---:|---:|
| 1 | 8.484 | 0.018 | 16.199 |
| 2 | 23.289 | 0.050 | 67.186 |
| 4 | Not reached before old 120-second timeout | 0.210 | 288.182 |

The four-call timeout was raised to 300 seconds. All three rfort attempts reached
the same generated-storage initialization error (`@alloc_block_7316`); all three
gfortran object compilations passed. These are still failed-compilation timings,
not successful builds or runtime validation. The four-call result does not measure
a before/after speedup because its previous run stopped before semantic analysis.

A fresh one-call run of the saved old compiler took 6.061 seconds in math checks
and 37.145 seconds overall. Unchanged stages varied substantially between runs;
do not attribute the full overall timing difference to this change. The math pass
itself is now a small part of compilation. In the new four-call run, semantic
function-body checks took 60.046 seconds, and reduction lowering took 39.016 seconds.
Source span indexing, other semantic traversals, and repeated expansion remain
candidates for further work.

Reports, logs, and compiler/source snapshots:

- `target/2015-compat/fve-math-scoped/`
- `target/2015-compat/fve-math-old-recheck/`

Validation includes direct semantic tests for invalid math/random calls and nested
scalar/array and real/integer shadowing. The existing intrinsic generic integration
tests exercise interpreter execution, native execution, and reference comparisons.


## Indexed semantic source lookups

Semantic analysis now builds a first-occurrence identifier/token-span index once,
replacing repeated whole-source scans in diagnostic and declaration handling.
Case-insensitive lookup, first-occurrence selection, and missing-name behavior
are preserved. Procedure analysis also borrows its procedure table instead of
cloning all procedure bodies before iteration.

The following sequential single-run measurements use the same release development
settings and module snapshots as the scoped math-check measurements above.

| Stats calls | Previous source/interface stage (s) | Indexed stage (s) | New semantic total (s) | New attempt total (s) |
|---|---:|---:|---:|---:|
| 1 | 0.663 | 0.031 | 3.010 | 21.939 |
| 2 | 3.645 | 0.064 | 12.024 | 67.099 |
| 4 | 4.789 | 0.061 | 53.836 | 232.173 |

All rfort probes still reach the generated-storage initialization error; all
three gfortran object compilations pass. Timing variability remains substantial:
the new one-call total is higher than the previous run despite faster source
lookups, while unchanged reduction lowering is faster in the four-call run.
These results demonstrate a stage-level improvement, not a controlled overall
speedup or successful compilation. Logs, source/compiler snapshots and the report
are in `target/2015-compat/fve-token-index/`.

Validation: the Rust lookup-equivalence test passes, as do all 23 integration tests
in `test_source_diagnostics`, `test_declaration_warnings`, and
`test_intent_spellings`.

Next target: the nested procedure-body checker still clones its symbol table for
every scope. The `sema.function_bodies` stage (which also includes subsequent
top-level checks) takes 2.119, 9.379, and 33.992 seconds in these runs. Scoped
insert/restore handling, with tests for nested declarations and assignment/return
type diagnostics, is the next candidate. Undefined-call checking and expansion
costs also remain significant.


## Scoped procedure-body type checking

The nested procedure checker now borrows its type environment, records local
scalar/array declarations, and restores them on scope exit. BLOCK no longer makes
an extra copy before recursive checking; DO temporarily inserts an otherwise
missing loop-index type and removes it afterward. Existing assignment, return,
INTENT, and read-tracking checks remain in place.

Sequential single-run results with the same module snapshots and optimized
development build settings:

| Stats calls | Previous function-body stage (s) | Scoped stage (s) | Previous semantic total (s) | New semantic total (s) | New attempt total (s) |
|---|---:|---:|---:|---:|---:|
| 1 | 2.119 | 0.811 | 3.010 | 1.797 | 22.731 |
| 2 | 9.379 | 3.884 | 12.024 | 7.707 | 79.627 |
| 4 | 33.992 | 10.315 | 53.836 | 20.907 | 187.346 |

The function-body stage also includes subsequent top-level checks. Other stages
still vary substantially: the first two complete attempts are slower than the
previous runs despite faster semantic checking. Do not infer a controlled overall
speedup from these single runs. All three rfort probes still reach the same
`@alloc_block_7316` initialization error; all gfortran object compilations pass.
The full FVE program has not been shown to compile successfully.

Reports and compiler/source snapshots: `target/2015-compat/fve-procedure-scopes/`.

Validation: both focused Rust tests pass, including six procedure-scope cases
covering nested and sibling blocks, array/scalar shadowing, loops, and invalid
assignments before/after restoration. All 23 integration tests in
`test_function_scope_metadata`, `test_function_result_types`,
`test_select_case_function_result`, and `test_intent_spellings` pass.

Undefined-call checking takes 0.800/3.358/9.945 seconds for one/two/four calls.
At that point it still cloned procedure environments and was the next semantic
optimization candidate. Unlike the type-only checker, it introduces implicit
symbols during expression traversal; any scoped replacement must preserve those
rules and prevent declarations leaking between procedures or blocks.


## Scoped undefined-call checking

Undefined-call checking now records every symbol-table insertion (including
implicit declarations) and restores a checkpoint on procedure exit. It no longer
copies the entire enclosing table for each procedure. Top-level updates survive,
and explicit/implicit locals and parameter types do not leak into sibling scopes.
The set of statements traversed by this pass was not expanded.

| Stats calls | Previous undefined-call stage (s) | Scoped stage (s) | New semantic total (s) | New attempt total (s) |
|---|---:|---:|---:|---:|
| 1 | 0.800 | 0.030 | 0.953 | 30.602 |
| 2 | 3.358 | 0.022 | 1.569 | 42.403 |
| 4 | 9.945 | 0.053 | 7.379 | 111.567 |

These are sequential single-run measurements using the same module snapshots and
optimized development settings as above. The nonmonotonic one/two-call stage
timings and variations in unchanged stages reinforce that these are profiling
observations, not controlled speedup ratios. All three rfort attempts still reach
the same generated-storage initialization error; all gfortran object compilations
pass. The full FVE build has not been established to succeed.

Reports and compiler/source snapshots: `target/2015-compat/fve-undefined-scopes/`.

Validation: all four focused Rust tests pass (nested restoration, preservation of
top-level updates, procedure-local name isolation, and preceding regressions).
All 32 integration tests in `test_implicit_typing`, `test_external_calls`, and
`test_function_scope_metadata` pass.

Earlier lowering is now the larger target: the four-call run spends 63.156 seconds
in `array_calls::resolve`, including substring, allocatable, and function-array
handling; reduction lowering takes 16.455 seconds and section lowering 12.887.
These nested timings must not be added to their parent pass. Semantic analysis
takes 7.379 seconds, of which 6.792 is in the function-body/top-level-check stage.
The next investigation should profile repeated expansion and work within
array-call lowering, using the matched probes before attempting another full FVE
compile. The initialization error remains an independent correctness blocker.


## Shared CHARACTER lowering metadata

The dynamic substring pass now shares character-capacity, character-length, and
function-result maps until mutation; its read-only procedure-name set is also
shared. Removing a missing character binding no longer forces a copy. Module
storage keys are collected with one scan per scope, retaining the first matching
binding, instead of scanning all statements once per inherited deferred-length
character name.

| Stats calls | Previous dynamic substring stage (s) | Shared metadata stage (s) | New array-call stage (s) | New attempt total (s) |
|---|---:|---:|---:|---:|
| 1 | 3.677 | 0.072 | 9.191 | 18.027 |
| 2 | 7.054 | 0.156 | 14.701 | 39.827 |
| 4 | 17.097 | 0.454 | 56.571 | 110.593 |

Same separate module snapshots and optimized development build settings, sequential
single runs. All three rfort attempts still reach the generated-storage
initialization error; all gfortran object compilations pass. The four-call overall
time is essentially unchanged from the previous run because other stages vary.
The stage-level reduction does not establish a controlled overall speedup or a
successful FVE compilation.

Reports, logs and compiler/source snapshots: `target/2015-compat/fve-character-metadata/`.

All 22 integration tests in `test_substring_scope_sharing`,
`test_scalar_substrings`, `test_deferred_character_locals`, and
`test_parameter_substrings` pass. These cover runtime substring snapshots,
shadowing, procedure-local deferred CHARACTER, and host/module storage.

Next profiling targets within array-call lowering are allocatable finishing
(21.624 seconds for four calls) and function-array lowering (10.654 seconds).
The total array-call stage is 56.571 seconds; its nested timings must not be added
to it. Reduction and section lowering outside that stage take 19.147 and 11.833
seconds, respectively. Repeated expansion remains a broader investigation beyond
these environment-management optimizations.

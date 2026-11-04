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

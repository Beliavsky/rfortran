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

The default sweep runs the four original-module probes. To run just the strongest
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

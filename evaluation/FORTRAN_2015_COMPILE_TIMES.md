# Separate-compilation survey: Fortran 2015 programs

Measured on Windows on 2026-11-04. This survey broadens the FVE investigation to other main programs, without changing the compiler or the original sources.

## Main findings

- Of 16 selected programs, rfort compiled 9 main objects, failed on 3 mains, was blocked by 3 dependency failures, and timed out on 1 main.
- Gfortran compiled 15 main objects; the remaining program has a missing imported procedure.
- `xmomentum_system.f90` is the strongest new performance investigation: rfort exceeded 120 seconds after its modules had compiled, versus 0.304 seconds for gfortran.
- `xstats_vec.f90` and `xstats_tensor.f90` compile successfully but take approximately 44 and 37 seconds. These provide additional performance regression cases that are independent of FVE.

## Method

The runner reads ordered `*_files.txt` manifests, snapshots sources and INCLUDE files, compiles dependencies individually, and then compiles the main against those module interfaces. Compiler work directories are isolated. Identical dependency prefixes are cached within the run; failed prefixes are cached too. No linking or execution is performed.

Both compilers use optimization level 2. Gfortran also uses `-ffree-line-length-none`; rfort runs with `RFORT_TIME_PASSES=1`. Each invocation has a 120-second limit. These are individual diagnostic measurements, not repeated benchmark medians. The 16 manifests are a targeted sample, not a random sample of all programs in the directory.

The table measures **main object compilation only**, excluding module compilation. Compilers can distribute work differently between modules and mains: for example, one `statistics.f90` dependency compilation took about 21 seconds with gfortran and 2.85 seconds with rfort. Main-only timing ratios must not be interpreted as complete-build ratios. Successful object compilation does not establish runtime correctness.

## Results

| Manifest | Main | rfort status | rfort seconds | gfortran seconds (status) |
|---|---|---|---:|---:|
| `cxb_files.txt` | `xxcobyla_orig.f90` | passed | 6.478 | 0.246 (passed) |
| `acf_files.txt` | `xacf.f90` | passed | 0.386 | 0.307 (passed) |
| `ar1_files.txt` | `xar1.f90` | dependency_failed | not reached | 0.277 (passed) |
| `bcx_files.txt` | `xbox_cox.f90` | passed | 12.921 | 0.274 (passed) |
| `elm_files.txt` | `xelem_char.f90` | passed | 7.659 | 0.202 (passed) |
| `pse_files.txt` | `xprint_random_seeds.f90` | passed | 6.471 | 0.212 (passed) |
| `sim_files.txt` | `xsimple_interp_mv.f90` | failed | 4.309 | 0.268 (passed) |
| `qpt_files.txt` | `xquad_prog_temp.f90` | passed | 0.582 | 0.228 (passed) |
| `abc_files.txt` | `xstatistics_str.f90` | passed | 6.634 | 0.279 (passed) |
| `dsa_files.txt` | `x3d_stats.f90` | failed | 20.422 | 0.734 (passed) |
| `stt_files.txt` | `xstats_tensor.f90` | passed | 36.788 | 0.233 (passed) |
| `stv_files.txt` | `xstats_vec.f90` | passed | 43.825 | 0.235 (passed) |
| `tta_files.txt` | `xtest_stats.f90` | failed | 22.212 | 0.207 (passed) |
| `bvn_files.txt` | `xbivariate_normal.f90` | dependency_failed | not reached | 0.168 (failed) |
| `mom_files.txt` | `xmomentum_system.f90` | timeout | 120.055 | 0.304 (passed) |
| `reg_files.txt` | `xreg_range.f90` | dependency_failed | not reached | 0.622 (passed) |

`dependency_failed` means the main was not attempted. Timeout elapsed time can slightly exceed the configured limit.

## Where time goes

Selected phase durations in seconds:

| Main | Significant phases |
|---|---|
| `xmomentum_system.f90` | Initialization 42.490; IR lowering 29.295; array-call resolution 11.906; timed out after entering codegen |
| `xstats_vec.f90` | Codegen 16.162; initialization 8.614; IR lowering 6.787; derived-type resolution 4.319; array-call resolution 4.231 |
| `xstats_tensor.f90` | Codegen 12.652; initialization 6.907; derived-type resolution 5.812; IR lowering 4.697 |
| `xbox_cox.f90` | Derived-type resolution 7.937 of 12.921 total |
| `xxcobyla_orig.f90` | Codegen 4.214 of 6.478 total |

Phase times can be nested and must not all be added together. The momentum log enters `codegen` without reaching the finer function construction/emission markers. This does not establish a register-allocation bottleneck.

## Momentum size follow-up

Copied variants changing only `nobs = 1000` to 10 and 100 both still timed out at 120 seconds in rfort, after entering codegen. Merely reducing this extent therefore does not resolve the slowdown; this experiment does not isolate the cause.

| nobs | rfort | gfortran -O2 |
|---:|---:|---:|
| 10 | timeout (120.063 s) | 0.392 s |
| 100 | timeout (120.057 s) | 0.326 s |
| 1000 (original survey) | timeout (120.055 s) | 0.304 s |

The follow-up used `tests/run_fve_compile_probes.py` with the previously built momentum module interfaces. That runner uses rfort's default optimization and gfortran `-O0`; gfortran was then separately rerun with `-O2` for the table above. The original runner results are retained in `target/2015-compat/momentum-size-survey-20261104/report.json`; the additional reference commands and measurements are in `gfortran-o2.json` in the same directory. These copies were compiled only; reducing `nobs` without changing the other parameters is not asserted to produce valid runtime tests.

## Compilation failures

- `ar1`: dependency `random.f90` reports an uninitialized `fn_val` read.
- `sim`: `xsimple_interp_mv.f90` reports an unsupported expression in a numeric array reduction.
- `dsa`: `x3d_stats.f90:33` reports an unknown vector subscript type for output of `xx(i1,i2,:)`.
- `tta`: compiling `xtest_stats.f90` reports an uninitialized generated `@alloc_block_3212` read in imported `statistics.f90:1879`, at a WRITE statement.
- `bvn`: rfort fails in dependency `print_corr_mat_stats.f90`, resolving generic `cbind` with an unknown argument type. Gfortran also fails the main because `bivariate_normal` is not found in `random_normal_mod`; this is not a clean whole-program comparison.
- `reg`: dependency `reg_stats.f90` fails generic resolution of `solve_lsq_stats`.

The other five failing/blocked rfort groups compile all their ordered objects with gfortran. Exact diagnostics and commands are retained in the JSON and logs.

## Recommended next work

1. Reduce the momentum example by removing individual calls and imported procedures, preserving its module context. Determine what expands before initialization and IR lowering, and what happens at the beginning of codegen.
2. Use the successful statistics-vector and statistics-tensor mains to verify that changes improve multiple programs.
3. Keep Box-Cox and COBYLA as distinct phase-specific cases, and ACF as a fast control. Continue checking FVE after each general optimization.

Prefer a small suite of these cases over repeatedly measuring only FVE. Establish a specific cause before changing compiler lowering.

## Reproduce and inspect

From the repository root, with gfortran on PATH:

```cmd
python tests\run_2015_compile_survey.py --output target\2015-compat\main-survey-new
```

Choose a new output directory for each run. To select particular groups:

```cmd
python tests\run_2015_compile_survey.py --output target\2015-compat\main-survey-selected mom stv stt bcx
```

Options include `--root`, `--compiler`, `--reference`, and `--timeout`. The runner compiles only; it never executes the Fortran programs.

Local artifacts (ignored by Git):

- `target/2015-compat/main-survey-20261104/report.json`: full results, timings, commands, hashes and log paths.
- `target/2015-compat/main-survey-20261104/sources/`: source and manifest snapshots.
- `target/2015-compat/main-survey-20261104/cases/`: main compilation logs and interfaces.
- `target/2015-compat/main-survey-20261104/modules/`: dependency compilation logs and interfaces.

The run contains 151 unique compile invocations and 44 reused dependency results.

Compiler snapshot SHA-256: `7827b4c85c28446089c887178aae93b9cd64170a4e2f5247666b2a65fe5f6c5d`.

Reference: `GNU Fortran (GCC) 17.0.0 20260510 (experimental)`.

## Momentum reduction and module-storage scan follow-up (2026-11-04)

Removing the two `print_stats` calls from a copied momentum main reduced its
compilation to 4.033 seconds with the saved compiler. Removing the random-normal
calculation or replacing the trading-system call did not remove the slow path.
Those exploratory runs overlapped part of a compiler rebuild and are not used
as controlled timing comparisons.

The new [small statistics driver](../tests/programs/xmomentum_compile_stats.f90)
contains just a fixed 10-by-2 matrix and one `print_stats([iimean], ...)` call.
It requires the external `kind_mod` and `statistics_mod` interfaces. This isolates
the problematic call path without copying the external statistics library into
this repository.

The native module-storage declaration scan used to clone its complete visible
type map at every block, branch and loop. It now saves and restores only the
declarations introduced by each scope. This preserves inherited and shadowed
types without repeatedly copying unrelated bindings. The new
`codegen.module_data` timer isolates the scan; it takes 0.018 seconds on the full
momentum main in both patched runs.

### Measurements and variability

These are sequential compile-only runs against the same saved momentum module
interfaces, using default rfort optimization level 2 and pass timing. The full
main was allowed 180 seconds rather than the original survey's 120 seconds.
No compiler rebuild was running during the measurements in this table.

| Main/probe | Saved compiler seconds | Patched compiler seconds |
|---|---:|---:|
| Small `print_stats` driver | 28.681 | 22.360 |
| Full momentum main | 90.648 | 109.195; repeat 68.560 |
| Statistics vector main | 15.810 | 17.018 |
| Statistics tensor main | 13.178 | 16.268 |

The historical 37?44-second statistics timings are **not** a reliable before/after
baseline: fresh runs of the saved compiler also finished much sooner. The matched
statistics runs do not demonstrate an overall speedup. Keep all measurements,
rather than choosing only the fastest patched run. The timing variation is not
explained by this experiment and these are not benchmark medians.

The momentum phase breakdown provides more specific evidence:

| Phase | Saved compiler | Patched first run | Patched repeat |
|---|---:|---:|---:|
| Initialization analysis | 24.700 | 43.896 | 22.927 |
| IR lowering | 18.118 | 26.428 | 17.643 |
| Code generation, including module-storage scan | 36.008 | 18.318 | 17.462 |

The patch does not change initialization analysis or IR lowering. Their variation
explains why the first patched whole-compilation time was worse despite less time
in code generation. The small driver's code-generation phase fell from 11.501 to
5.789 seconds. Further repeated, controlled measurements are needed before
claiming a stable overall speed ratio.

### Validation and remaining work

All 18 module-scope and 14 separate-module-compilation tests pass against the
rebuilt release compiler. The added scope regression exercises saved INTEGER,
REAL and CHARACTER values in nested/sibling blocks across repeated calls, with
interpreter, native O0/O2 and gfortran checks.

The small statistics driver links against the saved module objects and prints
two means of 1.0000, matching gfortran. The existing FVE scalar statistics probe
also compiles (56.471 seconds), links and prints 0.01 for input 3, numerically
matching gfortran. The **full FVE main was not rerun** in this follow-up; its last
recorded blocker remains initialization-analysis time. Neither the full momentum
application nor the random statistics mains was executed in this timing check.

Next priorities remain initialization environment copies/joins and IR-lowering
type-map copies. Both passes still do substantial work before native code
generation. Use the small driver, the full momentum main, statistics mains and
FVE probes together when checking subsequent optimizations.

### Artifacts and reproduction

Local reports are under `target/momentum-reduction/`:

- `baseline/`: exploratory call-removal variants.
- `small-before/`, `full-before/`, `statistics-before/`: saved-compiler measurements.
- `after/`, `after-repeat/`: patched-compiler measurements and pass logs.
- `execution.json`: link/run commands and outputs for the two checked probes.

Saved compiler SHA-256: `7827b4c85c28446089c887178aae93b9cd64170a4e2f5247666b2a65fe5f6c5d`.
Patched compiler SHA-256: `a20a12d80a8eae73abf0ea455e7c9cfa0f5d24dd024348cb35f7d2c52b87e664`.

With the earlier survey's module interfaces available, run from Windows CMD:

```cmd
python tests\run_fve_compile_probes.py --modules target\2015-compat\main-survey-20261104\cases\rfort\mom_files --output target\momentum-reduction\new-run --timeout 180 tests\programs\xmomentum_compile_stats.f90
```

Choose a fresh output directory. Add `--compiler PATH` to measure another binary.
The helper's optional gfortran comparison uses `-O0`; it was used only as a
reference for the small-driver baseline, not to claim matched optimized compiler
speed. The execution verification compiled its gfortran probes with `-O2`.

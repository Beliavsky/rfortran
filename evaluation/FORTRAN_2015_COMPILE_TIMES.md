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

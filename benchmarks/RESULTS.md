# rfort performance relative to gfortran

These are alpha-stage measurements, recorded on **2026-11-04**, not a claim of production readiness. Rfort was slower than gfortran on all twelve execution kernels in these runs. Compilation of some larger programs remains much slower and can time out. Both strengths and limitations should be evaluated on your own workload.

## Execution speed

All 12 kernels passed the suite's checksum and error checks with both compilers, in both configurations below. That validates these cases within their documented tolerances, not general compiler correctness.

Times are median **kernel execution seconds**, excluding compilation, process startup, initialization, validation and printing. A ratio above 1 means rfort took longer. `short` means at least one median was below 0.02 seconds; its ratio is deliberately omitted. Use the large profile to investigate those cases before drawing quantitative conclusions.

### Default runtime checks

Both compilers use `-O2`. Rfort enables runtime checks by default; gfortran does not. This compares their normal configurations, not equivalent checking overhead.

| Kernel | rfort seconds | gfortran seconds | rfort / gfortran |
|---|---:|---:|---:|
| `integer_sum` | 0.051213 | 0.027022 | 1.90 |
| `scalar_recurrence` | 0.101283 | 0.061860 | 1.64 |
| `procedure_calls` | 0.151247 | 0.062851 | 2.41 |
| `pi_integral` | 0.022226 | 0.009546 | short |
| `vector_loop` | 0.148401 | 0.090883 | 1.63 |
| `vector_array` | 0.138665 | 0.087101 | 1.59 |
| `dot_product` | 0.385852 | 0.174119 | 2.22 |
| `array_reductions` | 0.746682 | 0.478853 | 1.56 |
| `matmul_loop` | 0.047953 | 0.013879 | short |
| `matmul_intrinsic` | 0.047618 | 0.002785 | short |
| `stencil_2d` | 0.108959 | 0.028089 | 3.88 |
| `allocation_fill` | 0.088539 | 0.026185 | 3.38 |

### Runtime checks disabled

Rfort uses `-O2 -nocheck`; gfortran uses `-O2`. These are independent runs, so changes between the tables include machine/timing variation as well as checking overhead. Disabling checks is not necessarily faster in every measured case.

| Kernel | rfort seconds | gfortran seconds | rfort / gfortran |
|---|---:|---:|---:|
| `integer_sum` | 0.048074 | 0.028841 | 1.67 |
| `scalar_recurrence` | 0.105042 | 0.064031 | 1.64 |
| `procedure_calls` | 0.140074 | 0.060386 | 2.32 |
| `pi_integral` | 0.018643 | 0.008258 | short |
| `vector_loop` | 0.151896 | 0.091854 | 1.65 |
| `vector_array` | 0.179141 | 0.099840 | 1.79 |
| `dot_product` | 0.386343 | 0.171501 | 2.25 |
| `array_reductions` | 0.586625 | 0.479014 | 1.22 |
| `matmul_loop` | 0.045795 | 0.014047 | short |
| `matmul_intrinsic` | 0.049515 | 0.002929 | short |
| `stencil_2d` | 0.080323 | 0.017169 | short |
| `allocation_fill` | 0.077418 | 0.025480 | 3.04 |

## Measurement conditions

- Windows 11 x86-64, Intel Core i7-1185G7 at a nominal 3.00 GHz, 8 logical CPUs, approximately 16 GiB RAM. CPU frequency was not pinned.
- Rfort `0.1.0-alpha.1`, host `x86_64-pc-windows-msvc`, using the existing release executable; no compiler rebuild was performed for this page.
- Gfortran `GNU Fortran (GCC) 17.0.0 20260510 (experimental)`. This is an experimental GCC build, not a released gfortran baseline.
- `medium` sizes, seed 3, one warmup and five measured runs per executable. The harness alternates compiler order between measured rounds.
- Thread-limit environment variables are set to 1. No external BLAS, fast-math, or native-CPU tuning was requested.
- Source files, sizes, tolerances, exact commands, executable/source hashes, individual samples, build times and process wall times are retained in the JSON reports below.

These results describe one Windows machine and one compiler snapshot. They do not establish performance on Linux/macOS, other processors or larger applications. The checking modes are not identical across compilers. Short kernels, system load, CPU frequency and caching can affect measurements. No single aggregate speed claim is made across this small, deliberately varied suite.

Rfort executable SHA-256 (both execution runs and the compilation survey):

```text
7827b4c85c28446089c887178aae93b9cd64170a4e2f5247666b2a65fe5f6c5d
```

The executable hash identifies the measured build; the version string alone does not identify a source revision. Refresh these results when publishing a different release binary.

## Compilation speed and known slow cases

A separate 16-program survey compiled dependencies individually and then compiled each main program against the saved module interfaces. Selected results follow; the [complete survey](../evaluation/FORTRAN_2015_COMPILE_TIMES.md) includes every case and diagnostic category.

| Main program | rfort seconds | gfortran seconds |
|---|---:|---:|
| `xacf.f90` | 0.386 | 0.307 |
| `xxcobyla_orig.f90` | 6.478 | 0.246 |
| `xbox_cox.f90` | 12.921 | 0.274 |
| `xstats_tensor.f90` | 36.788 | 0.233 |
| `xstats_vec.f90` | 43.825 | 0.235 |
| `xmomentum_system.f90` | timeout at 120 | 0.304 |

These are **single main-object compilation measurements**, not full builds or execution times. Both compilers used optimization level 2; rfort had pass timing enabled. Dependencies, linking and program execution are excluded. Different compilers distribute work differently between modules and their users, so these figures must not be used as full-build speed ratios.

Across the complete survey, rfort compiled 9 main objects, failed on 3 mains, encountered 3 dependency failures and timed out on 1 main. Gfortran compiled 15 main objects; one failed due to a missing imported procedure. This was a targeted sample from `C:\fortran\2015`, not a representative census. The external source collection is not bundled with this repository.

## Reproduce the execution measurements

From the repository root in Windows CMD, with gfortran on PATH:

```cmd
python benchmarks\run.py --rfort target\release\rfort.exe --size medium --runs 5 --warmups 1 --checks default
python benchmarks\run.py --rfort target\release\rfort.exe --size medium --runs 5 --warmups 1 --checks none
```

Each invocation creates a new report directory under `target/benchmarks`. The runner uses the supplied binary; it does not rebuild it. To test a release package, substitute the path to its `rfort.exe`. To measure a newly built compiler, first run `cargo build --release --bin rfort`. Use `--size large` for kernels marked `short`, and avoid concurrent compiler builds or other heavy workloads during measurement.

See the [benchmark guide](README.md) for algorithms, correctness checks, additional flags and the meaning of each timing. The [compilation survey](../evaluation/FORTRAN_2015_COMPILE_TIMES.md#reproduce-and-inspect) gives its separate reproduction commands and external-source requirements.

## Saved measurement data

These copies are stored in the repository so the evidence remains available without the local `target` directory:

- [Default-check execution report](results/2026-11-04/default.json)
- [Unchecked execution report](results/2026-11-04/none.json)
- [Compilation survey records](results/2026-11-04/compile-survey.json), with duplicate nested invocation records removed

The reports retain original local paths as provenance; those paths are not downloadable artifacts. Executables and external Fortran sources are not included. The execution reports contain build diagnostics and raw timing samples; the compilation records contain phase timings and first errors, with full logs remaining in the local survey directory.

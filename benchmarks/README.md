# rfort / gfortran benchmarks

This independent suite measures correctness, kernel execution time, process wall
time, and compilation time. It uses existing compiler executables and never runs
Cargo or modifies compiler source files. The twelve Fortran kernels are original
MIT-licensed code; no external benchmark sources or libraries are required.

From the repository root in Windows CMD:

```cmd
python benchmarks\run.py --size smoke --runs 1 --warmups 0
python benchmarks\run.py --size medium
```

The smoke profile checks compatibility and correctness. Its timings are usually
too short for useful comparisons. Start performance measurements with `medium`;
use `large` when kernels still complete too quickly. The report suppresses ratios
when either median kernel duration is below 0.02 seconds (configurable with
`--min-seconds`). It still records the raw measurements. A correct, short-running
case is marked `passed (short timing)`, not a compiler failure.

The default compilers are `target\release\rfort.exe` and `gfortran` on PATH.
Override them explicitly if needed:

```cmd
python benchmarks\run.py --rfort C:\path\to\rfort.exe --gfortran C:\path\to\gfortran.exe --size medium
python benchmarks\run.py --compilers gfortran --bench "vector_*" "matmul_*"
python benchmarks\run.py --list
```

The runner snapshots the rfort executable into each run's output directory so a
concurrent compiler rebuild does not replace it during the run. It does not copy
gfortran, whose executable depends on its toolchain installation. This is not a
complete snapshot of external linkers, runtime caches or system libraries. Avoid
changing those during a measurement run.

Each run gets a fresh directory under `target\benchmarks\` containing:

- `report.json`: versions, executable hashes, source hashes, machine information,
  settings, exact build commands, build logs, correctness results, all measured
  samples and warmups, medians, spread, and process wall times.
- `summary.md`: a comparison table and diagnostics for failed cases.
- A separate directory per compiler and benchmark, with copied Fortran sources,
  generated configuration module, executable, and build logs.

Use `--output target\benchmarks\my-run` to choose a destination. An existing
nonempty directory is rejected so previous results are preserved. Failed cases
do not stop the remaining benchmarks. The final exit code is 1 if any case
failed or a selected compiler was unavailable; `--allow-failures` keeps the
report but returns zero, which can be convenient while collecting compiler gaps.

## Kernels

| Kernel | Purpose |
|---|---|
| `integer_sum` | INTEGER(64) modular accumulation |
| `scalar_recurrence` | Dependent REAL(64) arithmetic |
| `procedure_calls` | The same recurrence through a pure scalar function |
| `pi_integral` | Numerical integration using a midpoint rule |
| `vector_loop`, `vector_array` | Equivalent explicit-loop and array-syntax updates |
| `dot_product` | DOT_PRODUCT with an input changed between repetitions |
| `array_reductions` | SUM, MAXVAL and MINVAL |
| `matmul_loop`, `matmul_intrinsic` | Equivalent matrix products with loops and MATMUL |
| `stencil_2d` | Two-dimensional Jacobi update with an analytic reference solution |
| `allocation_fill` | Allocate, initialize, reduce, and deallocate; not pure allocation latency |

Sizes and tolerances are in `manifest.json`. `smoke`, `small`, `medium`, and
`large` use the same algorithms with different sizes/repetition counts. Matrix
loops use column-major traversal. The suite starts with single-threaded CPU
work; the child environment sets OMP, OpenBLAS, MKL and vecLib thread limits to
one. There is no external BLAS, MPI, OpenMP code, fast-math or native-CPU tuning
in the default configuration.

## Measurement and correctness

Every program reads a seed from standard input before timing. The runner supplies
3 by default (`--seed` accepts 3 through 20). Data-dependent inputs and feedback
between repeated operations help prevent the computation from being removed or
hoisted wholesale. Source-level equivalence does not prohibit legitimate compiler
optimizations: inlining the procedure-call kernel is an intended optimization.

Programs measure the computational region with SYSTEM_CLOCK using a selected
64-bit integer kind. Initialization, output, and correctness checks are outside
that region. The runner also measures complete process wall time, which includes
startup and those extra activities. Compilation is separate and includes linking;
its timing can depend on toolchain/runtime caches. No cache-clearing is performed.

Both the independently computed Python checksum and the program's error metric
must pass for every warmup and measured execution. References use mathematical
formulas, periodic sums, or analytic solutions rather than accepting gfortran's
answer as authoritative. Floating-point comparisons use documented per-case
tolerances, not text equality or identical output formatting. The INTEGER(64)
checksum must match exactly. NaN, infinity,
missing or duplicate measurement lines and negative measurements are rejected.
Kernel times exceeding complete process wall time beyond a small clock tolerance
are rejected as well, so a gross SYSTEM_CLOCK error cannot produce a speed ratio.

The runner warms each executable once, then measures five fresh process runs by
default. It alternates compiler order between measured rounds. Reports include
median, minimum, maximum and standard deviation. A ratio above one means rfort
took longer. Do not treat smoke results or ratios from different machines,
sizes, flags or checking modes as comparable. Keep other heavy work, including
compiler builds, off the machine during serious performance measurements.

Both compilers receive `-O2` by default; this does not mean their optimization
passes or runtime safety checks are equivalent. Default runtime checks remain in
effect. Record any alternative checking or optimization configuration separately:

```cmd
python benchmarks\run.py --size medium --rfort-flag=-O0 --gfortran-flag=-O0
python benchmarks\run.py --size medium --no-default-flags --rfort-flag=-O2 --gfortran-flag=-O3
python benchmarks\run.py --size medium --runs 7 --warmups 2 --timeout 60 --compile-timeout 180
```

Use `--flag=value` spelling when a flag value starts with `-`. Add one option per
compiler flag. The harness uses subprocess argument lists, not shell command
strings, and does not call the batch wrappers that rebuild rfort.

Statuses distinguish `unavailable`, `compile_failed`, `compile_timeout`,
`run_failed`, `run_timeout`, `invalid_output`, `wrong_answer`, and `passed`.
Failed or incorrect programs never receive performance ratios. Reduce compiler
failures to separate regression tests; keep the benchmark algorithm intact when
fixing compiler gaps. Larger cases may also expose storage or resource limits.

Run the harness's own tests without building either compiler:

```cmd
python -m unittest discover -s benchmarks -p test_runner.py
```

External suites, such as those in the
[Fortran-code-on-GitHub benchmark list](https://beliavsky.github.io/Fortran-code-on-GitHub/#benchmarks-and-benchmarking),
can be added later after checking their licenses and dependencies. Preserve their
upstream revision and document adaptations when importing them.

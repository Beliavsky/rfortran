# Parallel DO CONCURRENT benchmarks

This suite compares the same Fortran program built serially and with
`--parallel`, using one, two, and four threads by default. It runs an existing
rfort executable and never builds or changes the compiler.

From the repository root in Windows CMD:

```cmd
python benchmarks\run_parallel.py --size smoke --runs 1 --warmups 0
python benchmarks\run_parallel.py --size small medium --runs 3 --warmups 1
python benchmarks\run_parallel.py --size medium --checks none
```

The default compiler is `target\release\rfort.exe`. Use `--rfort PATH` to select
another executable; batch wrappers are rejected. The harness snapshots that
executable so a concurrent rebuild cannot replace it during the sweep.
Avoid other heavy work during performance measurements.

## Workloads and sizes

| Kernel | Work measured |
|---|---|
| `independent_math` | Independent SQRT/SIN updates to a runtime-sized array |
| `scalar_reduce` | A double precision scalar sum |
| `array_reduce` | An eight-element double precision array sum |
| `complex_reduce` | A four-element complex array sum and a complex scalar product |
| `matrix_loop` | Two concurrent indices updating a column-major array |
| `section_results` | PURE array-valued function calls and disjoint column writes |
| `workspace_calls` | PURE calls with read-only sections and private automatic arrays |

Every repetition contributes to the final result; array updates accumulate
rather than repeatedly overwriting unobserved output. Complex multiplication
uses imaginary factors and a nontrivial starting value to check coupled
component arithmetic. Kind parameters use `kind(0d0)`.

| Size | Base elements | Timed repetitions |
|---|---:|---:|
| `tiny` | 128 | 1 |
| `smoke` | 10,000 | 1 |
| `small` | 100,000 | 2 |
| `medium` | 1,000,000 | 4 |
| `large` | 5,000,000 | 8 |

`array_reduce`, `complex_reduce`, `section_results`, and `workspace_calls` use one tenth of
the base size for small/medium/large, with a minimum of 9,000. The runtime seed
(3 by default) is added to the element count. `matrix_loop` has four times as
many concurrent iterations as columns. Exact dimensions and repetition counts
are saved per result.

`tiny` deliberately exercises the runtime's small-range serial path. `smoke`
checks compatibility and dispatch; its timings are often too short to compare.
Use medium/large for performance measurements, especially when short timings
are marked with `*`.

```cmd
python benchmarks\run_parallel.py --list
python benchmarks\run_parallel.py --size tiny smoke --bench "*reduce" --runs 1 --warmups 0
python benchmarks\run_parallel.py --size medium --threads 1 2 4 8 --bench independent_math workspace_calls
python benchmarks\run_parallel.py --size medium --cold-pool
```

Parallel one-thread execution is always included, even when `--threads` omits
1. It exposes differences from the ordinary serial lowering; the difference
is not solely worker-pool overhead. The Threads column is the requested count;
actual multi-thread dispatch counts are recorded by the probe. Other available options include `-O`,
`--seed`, `--timeout`, `--compile-timeout`, `--min-seconds`, `--output`, and
`--allow-failures`. An existing nonempty output directory is rejected.

## Correctness and worker verification

Each program reports elapsed kernel time, a checksum, and an error metric.
The harness independently computes the expected checksum in Python using
periodic sums or closed-form expressions. Programs also check individual
array elements outside the timed region. NaN/infinity, malformed output,
incorrect checksums, excessive element errors, and implausible clock values
are failures.

Both builds use `-O2` and the same runtime-check settings by default.
`--checks none` disables rfort's initialization and bounds checks for both.
These configurations are saved separately; do not combine their ratios.

The parallel build must report a native worker and no serial fallback. Each
execution configuration gets a separate untimed probe with
`RFORT_PARALLEL_TRACE=1`. For ranges of at least 4096 iterations, the probe must
confirm the requested thread count when it exceeds one. Small ranges and
single-thread configurations must not report a multi-thread dispatch. The
threshold matches the current runtime; a future policy change may require
updating the harness. Fallbacks and failed dispatch checks are recorded as
failures and receive no speedup.

Timed processes have tracing and pass-dump diagnostics disabled. Unrelated
OpenMP/BLAS thread limits are set to one. Probes, process warmups, and timed
samples are separate in the report. Configuration order rotates between
measurement rounds to reduce ordering bias.

## Timing interpretation

By default, each program performs one untimed iteration, discards its result,
and then times the requested repetitions. This warms the worker pool and
touches buffers before measuring steady-state work. `--cold-pool` skips this
initial iteration and includes first-use pool creation in the kernel time.
The separate dispatch probe and process warmups cannot warm the pool in a
later process; the in-program warmup is what does that.

Kernel time excludes initial allocation, input, validation, and printing.
Worker-private allocation and reduction combination within a timed construct
are included. Whole-process wall time includes startup, the in-program warmup,
validation, and shutdown. Compilation is measured separately, with build
commands and diagnostic logs saved.

The console aligns decimal points and reports kernel and process medians plus
`serial kernel median / configuration kernel median`. A speedup above one
means faster execution than the serial build. `*` marks a ratio involving a
median below `--min-seconds` (0.02 seconds by default); treat it as preliminary.
Zero timings and failed cases receive no ratio. No speedup is guaranteed:
short loops, memory bandwidth, temporary allocation, and reduction overhead
can outweigh parallel execution.

## Reports and tests

Each run creates `target\benchmarks\parallel\<timestamp>-<id>\` containing:

- `report.json`: compiler version/hash and snapshot, machine information,
  harness/source hashes, exact settings, build commands/times, dispatch probes,
  raw warmups/samples, correctness results, medians, spread, and speedups.
- `summary.md`: a table of statuses, median times, and speedups.
- Per-size/kernel/build directories with the exact sources, generated
  configuration module, executables, and compilation logs.

Results are saved incrementally; a failing case does not stop later kernels.
The exit status is nonzero when any configuration fails, unless
`--allow-failures` is supplied. An unavailable compiler remains an error.

Run harness tests without a Fortran compiler:

```cmd
python benchmarks\test_parallel_runner.py
```

### Compiler gap encountered during setup

Importing `dp` from the existing benchmark support module, where it is defined
with `selected_real_kind(15,307)`, currently causes the early parallel eligibility
check to reject captures using that kind. The kernels use a local
`dp=kind(0d0)` declaration to exercise the supported path. This is a compiler
limitation, not a restriction on valid Fortran. A small example is saved in
[`repros/imported_selected_kind.f90`](repros/imported_selected_kind.f90).

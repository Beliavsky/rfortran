# Compiler performance

For large inputs, measure compiler passes before changing optimization levels.
`-O0`/`-O2` control optimization of the generated program; they do not avoid
front-end argument association, specialization or initialization checking.

Use the release compiler for compile-time comparisons. A debug compiler is
useful for locating expensive passes, but its times are not comparable with an
optimized gfortran installation. Compare the same sources, flags and outcome:
time to a diagnostic is not time to a successfully generated object file.

## Command timings

Use `rf.bat --time -c xacv.f90` (or `rfort --time -c xacv.f90`) for a
concise compilation and overall wall-clock summary on stderr. With `--run`,
execution is timed separately. Compilation includes linking unless `-c` is
specified. Failed phases are marked; the child program exit status is preserved.
The overall time includes rfort startup, but excludes wrapper startup/rebuilds.
Repeat measurements with the same inputs, flags and compiler build.

## Optional pass timings

Set `RFORT_TIME_PASSES=1` to report compiler pass start/end times on standard
error. Start messages identify the active pass even if it has not finished.
Normal compilation remains quiet when this environment variable is absent.

Windows Command Prompt, from the repository root:

```cmd
set RFORT_TIME_PASSES=1
rf.bat -c xacv.f90 2>target\compiler-passes.txt
set RFORT_TIME_PASSES=
```

Linux/macOS:

```sh
RFORT_TIME_PASSES=1 ./target/release/rfort -c xacv.f90 2>target/compiler-passes.txt
```

`[pass]` messages give wall times for named passes. Nested pass times overlap:
do not sum a parent pass and its children. During long specialization sweeps,
`[work]` summaries report counts, cumulative wall times and longest calls for
environment construction, vector lowering, specialization and inlining.
Inlining and specialization measurements may nest, so these cumulative times
also overlap. This instrumentation is diagnostic, not a stable machine-readable
interface.

## Large-source specialization

The `xacv.f90` investigation found these sources of repeated work:

- Each recursive array-call scope rebuilt the complete procedure-intent table.
  The compiler now constructs it once per array-call resolution and shares the
  immutable table with vector lowering.
- Vector lowering recursively visited an entire subtree at every array-call
  scope, although array-call expansion itself already visits child scopes.
  Specialization now lowers vectors in one scope at a time. Standalone vector
  passes retain their recursive traversal.
- The final fixed-vector pass deep-copied its unchanged function-result and
  procedure-intent maps whenever it cloned a scope. Those maps are now shared
  too; declarations and variable shapes still have their own local scope.

The workload visits tens of thousands of generated scopes, so per-scope costs
matter. Early instrumented debug runs reduced the first expansion sweep from
15.217 seconds to 4.150 seconds after sharing interfaces, and to 2.714 seconds
in a later run with scope-local vector lowering. These preliminary runs were
not controlled whole-program benchmarks; other processes and profiling
overhead affect wall times. Logs are under `target/compile-profile-*.txt`
(ignored).

Correctness coverage includes `tests/test_vector_subscripts.py`,
`tests/test_vector_ranks.py`, `tests/test_select_case_calls.py`,
`tests/test_array_scope_metadata.py`, `tests/test_scalar_array_functions.py`
and `tests/test_compile_profile.py`. Checks cover interpreter and native O0/O2
execution, vector ranks through 15, selected-branch side effects, uninitialized
arguments, and vectors revealed after argument specialization inside branches
and loop conditions.

## End-to-end verification

On this Windows checkout, `gfortran -c xacv.f90` completed successfully in
9.596 seconds. A profiled release rfort run with the first two optimizations
was stopped at its 300-second limit during `vector_subscripts::finish`.
After sharing the final vector pass's immutable signature maps too, another
profiled release run was stopped at its 600-second limit in the same pass.
Neither rfort run produced an object or a compiler diagnostic before the limit.
These runs do not establish an overall compile-time speedup.

The final run recorded about 25.012 seconds and 61.436 seconds for the two large
array-call sweeps, 16.981 seconds for the intervening function expansion, and
17.075 seconds for output preparation after expansion. It visited approximately
187,000 array-call scopes. Final vector lowering is the largest unresolved
bottleneck in this measurement; the high-level timer covers both runtime and
fixed-vector lowering. A finer profile must distinguish these two parts before
attributing that bottleneck to one implementation.

The next performance work should focus on that pass and on scope-variable
metadata copying and rescanning of generated code. Sharing signature tables
removes known redundant copies, but does not remove all scope-map cloning or
the size of the specialized AST. Success means completing the large source,
not just reaching a later pass faster.

Exact commands, durations and timeout flags are in
`target/acv-repros/compile-performance/report.json`; the final pass log is
`rfort_final.txt` in the same directory (both ignored). Gfortran used its
default optimization level; rfort used its default level and profiling was
enabled. These results are diagnostic measurements, not a successful-object
benchmark comparison.

All 53 focused integration tests and 133 Rust unit tests pass with these
changes. The integration suite includes gfortran comparisons where applicable.
The rebuilt release also passes 19 vector, SELECT CASE and profiling tests;
see `target/compile-performance-release-tests.txt` (ignored).

## Run-time cost of checks

Generated programs keep their uninitialized-value and subscript checks at every
optimization level. These optimizations reduce their cost without removing them
where they can fail:

- A nest of DO loops with constant, nonempty bounds that assigns every element
  of a fixed-size array exactly once per index, without EXIT, CYCLE or RETURN,
  defines the whole array. Later reads need no initialization check. A
  variable that no statement of its scope can make undefined again (INTENT(OUT)
  association, ALLOCATE, storage transport and similar) also skips the flag
  stores of writes after it is fully defined.
- Subscript checks of explicit-shape and allocatable arrays are tested inline;
  the guard procedure is called only to report a failure. A subscript that is a
  DO variable of a loop with constant bounds, plus or minus constants, and that
  stays within the extent is not tested.
- A DO variable that its body cannot redefine is kept in a register. Its storage
  still receives every value, for callees and for its value after the loop.
- A whole-array assignment whose right side reads only other ordinary arrays,
  or the destination at the same element, is assigned without a temporary.
- MATMUL accumulates with the row loop innermost, so its operands and result
  are read with unit stride. Each element still sums its products in order.
- The IR optimizer unrolls a constant loop only if the unrolled body has at most
  128 statements. Before this limit, a check-free matrix-product nest unrolled
  every level and compilation did not finish.

`tests/test_checked_loop_performance.py` checks that partial nests, EXIT and
out-of-range subscripts are still reported.

On the medium benchmark profile (`python benchmarks/run.py --size medium`),
the rfort/gfortran kernel-time ratio of matmul_loop fell from about 480 to about
3, stencil_2d from 227 to about 4.5, vector_array from 10.6 to about 1.7,
allocation_fill from 10.4 to about 3.5, and matmul_intrinsic from 83 to about 18.
Timings on the measurement machine varied by a factor of two during the session,
so the ratios are from interleaved runs of both executables. The remaining gaps
come mainly from gfortran's SIMD vectorization and its blocked library MATMUL.

## Module-storage declaration scan

Before generating functions, the native backend scans their IR to declare module
and saved storage. The `codegen.module_data` pass timing isolates this work.
Its type environment is shared during traversal, saving and restoring only the
declarations introduced by each nested scope. This avoids copying all visible
types for every branch, loop and block in heavily specialized main programs.
Shadowed declarations are restored before visiting sibling scopes.

The standalone driver `tests/programs/xmomentum_compile_stats.f90` isolates a
`print_stats` call from the Fortran 2015 momentum main. It requires the external
`kind_mod` and `statistics_mod` module interfaces; it is not a self-contained test.
Use `tests/run_fve_compile_probes.py` with `--modules` and, for a gfortran
comparison, `--reference-modules` pointing to the appropriate compiled interfaces.

# Compiler performance

For large inputs, measure compiler passes before changing optimization levels.
`-O0`/`-O2` control optimization of the generated program; they do not avoid
front-end argument association, specialization or initialization checking.

Use the release compiler for compile-time comparisons. A debug compiler is
useful for locating expensive passes, but its times are not comparable with an
optimized gfortran installation. Compare the same sources, flags and outcome:
time to a diagnostic is not time to a successfully generated object file.

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

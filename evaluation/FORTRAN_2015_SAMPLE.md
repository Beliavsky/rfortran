# Fortran 2015 source-list sample: 2026-11-03

Sources: `C:\fortran\2015`

A [second sample of 18 other groups](FORTRAN_2015_SAMPLE2.md) records additional
compilation results, failures and a timeout using the existing release binary.
A [third sample of 18 groups](FORTRAN_2015_SAMPLE3.md) records further results,
source-list issues and two diagnostic probes using corrected copies.
A [targeted fourth sample of 24 groups](FORTRAN_2015_SAMPLE4.md) favors new
dependencies and avoids repeatedly encountered shared blockers.

This is a selected 22-group sample, not a sweep of all 2,148 source lists.
Compiler: rfort 0.1.0-alpha.1, Windows x86-64.
Reference: GNU Fortran (GCC) 17.0.0 20260510 (experimental).

The original sample performed compilation and linking only; no generated program
was executed. The follow-up below also records a standalone repro whose
gfortran executable was run.
Source-list order is preserved. rfort uses its default optimization; gfortran uses -O0 -ffree-line-length-none. Each compiler has an isolated workspace.
The rfort executable was snapshotted; the source tree and compiler code were not modified.

rfort SHA256: `2b099a73e5b6baadd94bb814a9752fa7250c702596d4940bd531627eb125070f`

| Source list | Files | Result | rfort seconds | gfortran seconds | First rfort diagnostic |
|---|---:|---|---:|---:|---|
| 000_files.txt | 1 | both_failed | 0.222 | 0.383 | error: unexpected keyword end: expected one of: percent, arrow, '=', '(' |
| 001_files.txt | 2 | both_compile | 1.322 | 2.157 |  |
| aaa_files.txt | 2 | both_compile | 1.880 | 2.563 |  |
| acf_files.txt | 5 | both_compile | 3.232 | 6.088 |  |
| chp_files.txt | 1 | both_compile | 1.783 | 2.443 |  |
| cla_files.txt | 1 | both_compile | 0.759 | 1.161 |  |
| eqv_files.txt | 1 | both_compile | 0.707 | 1.235 |  |
| ffo_files.txt | 1 | both_compile | 0.603 | 0.999 |  |
| fve_files.txt | 13 | rfort_failure | 34.036 | 34.173 | error: PARAMETER initializer must be a supported integer constant expression |
| gdt_files.txt | 1 | both_compile | 0.640 | 1.067 |  |
| int_files.txt | 1 | both_compile | 0.737 | 0.926 |  |
| irl_files.txt | 1 | both_compile | 0.552 | 0.910 |  |
| low_files.txt | 1 | rfort_failure | 0.143 | 0.954 | error: this vector subscript expression is not yet supported |
| mix_files.txt | 3 | rfort_failure | 0.211 | 1.647 | Error: CHARACTER array indices and bounds must currently be constant integers |
| oup_files.txt | 1 | both_compile | 0.880 | 1.102 |  |
| rcq_files.txt | 1 | both_compile | 0.748 | 1.109 |  |
| rcsv_files.txt | 8 | rfort_failure | 10.300 | 13.174 | error: PARAMETER initializer must be a supported integer constant expression |
| reg_files.txt | 25 | both_failed | 36.666 | 22.581 | error: constructor implied-DO index `j` requires a locally declared scalar INTEGER variable |
| rhp_files.txt | 1 | both_compile | 0.435 | 0.758 |  |
| rnd_files.txt | 1 | both_compile | 0.383 | 0.600 |  |
| saa_files.txt | 1 | both_compile | 0.338 | 0.704 |  |
| tnh_files.txt | 1 | both_compile | 0.341 | 0.605 |  |

Counts: {'both_failed': 2, 'both_compile': 16, 'rfort_failure': 4}

This table records the original sample. After correcting `reg_files.txt`, a
targeted rerun confirmed that gfortran builds that group while rfort still
fails; see the follow-up below. Combining that rerun with the other original
results gives 16 groups accepted by both, five rfort-only failures, and one
group rejected by both. The other 21 groups have not been rerun.

Full diagnostics and commands are in report.json and each case directory. A compilation failure is not automatically a compiler bug; inspect the reference result and check the source and required flags. Compilation success does not establish runtime correctness.


## Failures accepted by gfortran

- **low_files.txt**, `xlower_bound.f90:6`: `print *, ivec(lbound(ivec))`
  is rejected with "this vector subscript expression is not yet supported".
  The original source is only seven lines and is already a small repro. For a
  rank-one array, LBOUND without DIM supplies a rank-one integer vector.
- **mix_files.txt** (`kind.f90`, `mix_dist.f90`, `xmix_dist.f90`):
  "CHARACTER array indices and bounds must currently be constant integers",
  followed by "ncomp is not a previously defined integer PARAMETER". The
  diagnostic gives no source location. The module has derived objects with
  CHARACTER fields, ALLOCATABLE component arrays and display/allocation routines;
  the exact triggering interaction has not been isolated.
- **rcsv_files.txt**, `statistics.f90:1082`: `prob =
  bin_freq(xthresh,xx)/real(max(1,size(xx)),kind=dp)` is rejected as an unsupported
  integer PARAMETER initializer. Both the function result and the called
  function result have bounds derived from SIZE. The reported statement is an
  executable assignment, not a PARAMETER declaration; the diagnostic does not
  yet identify the underlying cause.
- **fve_files.txt**, `util.f90:8070`: the recursive assignment
  `xma = moving_average_xlen(xx,real(n,kind=dp))` receives the same PARAMETER
  initializer diagnostic. This is inside a REAL array-valued recursive function.
  Treating it as the same bug as the CSV failure requires further reduction.

The full commands, source hashes, copied sources and compiler diagnostics are
saved locally in `target/2015-compat/sample-20261103/`. The machine-readable
report is `report.json`; the compact table is `report.md`.

## Groups rejected by both compilers

- **000_files.txt**: `xhello.f90` contains a bare `foo` statement on line 4.
  gfortran reports an unclassifiable statement. This is not an rfort-only failure;
  rfort's diagnostic instead points at the following END statement.
- **reg_files.txt**, original run only: gfortran cannot find `statistics_mod.mod` while compiling
  `fit_graph_poly.f90`, and therefore cannot build `fit_graph_poly_mod` for the
  main program. The supplied list does not provide the missing statistics
  module in this clean build. Independently, rfort stops earlier in
  `gnuplot.f90:499`, demanding a locally declared INTEGER implied-DO index `j`.
  The corrected-list follow-up below resolves this dependency problem and
  confirms the implied-DO failure as an rfort-only compilation bug.

## Follow-up: corrected regression source list and gnuplot repro

On 2026-11-03, after the user corrected `reg_files.txt`, the 25-source group
compiled and linked successfully with gfortran (28.675 seconds). rfort still
failed (31.481 seconds) at `gnuplot.f90:499`:

```fortran
forall (i=1:n) yplot(i,:) = [yy(i),sum([(coeff(j+1)*xx(i)**j,j=0,mpow)])]
```

The diagnostic is `constructor implied-DO index j requires a locally declared
scalar INTEGER variable`, although the procedure explicitly declares
`integer :: i,j,n,mpow`.

The targeted rerun used the existing release executable, without rebuilding
the compiler. Its SHA256 was
`a89f046e71e2f63838bf988d04dabcba2f92bb2103b88df0612f712bbbcc76a0`,
which differs from the original sample's executable. Full source snapshots,
commands, diagnostics and machine-readable results are saved locally in
`target/source-list-compat/20261103T125731Z-3744aa/` (`report.json` and
`report.md`). Neither compiler's full regression executable was run.

A standalone repro is saved in
[xforall_constructor_implied_do.f90](../xforall_constructor_implied_do.f90).
It retains the assumed-shape numeric inputs, automatic local array, FORALL,
SUM and nested constructor implied-DO. The existing release rfort reproduces
the same diagnostic at line 17. gfortran compiles the repro, and running its
executable prints the expected rows:

```text
    10.0     6.0
    20.0    17.0
    30.0    34.0
```

The standalone compilation artifacts are under
`target/repro-forall-constructor/`. No compiler source was changed.

## Additional small reductions

Three exploratory reductions and their compilation logs are saved locally under
`target/2015-compat/sample-20261103/reduced-cases/`:

- `mixture_runtime_components`: both compilers accept it. This is not a repro
  for the full mixture-module failure.
- `nested_array_result`: both compilers accept it. Array-valued calls of this
  simple form alone do not reproduce the CSV failure.
- `recursive_array_result`: gfortran accepts it; rfort reports
  "recursive CHARACTER function moving_average_xlen is not yet supported",
  although the result is explicitly REAL. This is a separate reproducible
  diagnostic/code-generation problem exposed by reduction, not a reproduction
  of the full utility module's PARAMETER error.

No executable from the original sample was run, including the exploratory reductions. The 16 compilation
successes are not claims about runtime correctness or numerical accuracy.

## Repeat the comparison

The harness [tests/run_source_list_compat.py](../tests/run_source_list_compat.py)
reads one source path per nonblank line, preserves listed compilation order,
copies the sources and resolvable INCLUDE files into fresh directories, snapshots
rfort, and records both compilers' diagnostics. It does not rebuild rfort or
change the external source files. For the same 22 groups, from Windows CMD:

```cmd
python tests\run_source_list_compat.py --root C:\fortran\2015 --lists ^
  000_files.txt 001_files.txt aaa_files.txt acf_files.txt chp_files.txt cla_files.txt ^
  eqv_files.txt ffo_files.txt gdt_files.txt int_files.txt irl_files.txt low_files.txt ^
  oup_files.txt rcq_files.txt rhp_files.txt rnd_files.txt saa_files.txt tnh_files.txt ^
  mix_files.txt rcsv_files.txt fve_files.txt reg_files.txt --limit 22 --compile-timeout 90
```

This creates a fresh directory under `target/source-list-compat/`. Use
`--compiler` for another prebuilt rfort executable, `--lists "*_files.txt"`
with `--limit N` for another sample, or `--output` to choose a new/empty report
folder. Selection is alphabetical after matching the list names. gfortran must
be on PATH for reference compilation. The runner completes the sample despite
compilation failures; consult the report counts rather than its exit status to
assess compatibility. Harness-only checks are:

```cmd
python tests\test_source_list_compat.py
```

# Fortran 2015 second source-list sample: 2026-11-03

Sources: `C:\fortran\2015`

This sample adds 18 groups not included in the
[first sample](FORTRAN_2015_SAMPLE.md). Nine compile and link with both
compilers, eight fail with rfort, and one reaches rfort's 90-second limit.
All 18 compile and link successfully with gfortran. There were no missing
INCLUDE files or reference compiler timeouts.

Selection used six evenly spaced names from each of three alphabetically
sorted buckets of complete source lists: one or two sources, three to seven,
and eight to twenty. The selected small groups all contain two sources.
This broadens coverage but is not an exhaustive or random compatibility estimate.
The 18 groups contain 109 listed source entries, including shared dependencies.

Compiler: existing rfort 0.1.0-alpha.1, Windows x86-64. Reference:
GNU Fortran (GCC) 17.0.0 20260510 (experimental).
The harness snapshotted rfort without rebuilding it. No compiler source or
external source files were changed. Each group preserves source-list order and
uses separate clean build directories for the two compilers. rfort uses default
optimization; gfortran uses `-O0 -ffree-line-length-none`. Each compilation has
a 90-second limit; timings are observations rather than performance benchmarks.

Compilation and linking only; no generated program was executed.

rfort SHA256: `a89f046e71e2f63838bf988d04dabcba2f92bb2103b88df0612f712bbbcc76a0`

| Source list | Files | Result | rfort seconds | gfortran seconds | First rfort diagnostic |
|---|---:|---|---:|---:|---|
| bsg_files.txt | 4 | rfort_failure | 0.422 | 1.563 | error: recursive specialized subroutine calls are not yet supported |
| cde_files.txt | 2 | rfort_failure | 3.548 | 4.744 | Error: multidimensional array `@record$dates$month` requires a statically known shape |
| clt_files.txt | 17 | rfort_failure | 12.086 | 18.534 | error: PARAMETER initializer must be a supported integer constant expression |
| ffa_files.txt | 3 | both_compile | 0.717 | 1.553 |  |
| ffd_files.txt | 20 | rfort_failure | 47.695 | 35.353 | error: no matching specific procedure for generic `solve_lsq_stats` with argument types [Array(Scalar(DoublePrecision), 2), Array(Scalar(DoublePrecision), 1), Array(Scalar(DoublePrecision), 1), Scalar(Integer(Some(4))), Array(Scalar(DoublePrecision), 1)] |
| flu_files.txt | 2 | both_compile | 1.902 | 1.202 |  |
| knd_files.txt | 2 | both_compile | 0.449 | 0.683 |  |
| mma_files.txt | 8 | both_compile | 9.237 | 2.773 |  |
| mop_files.txt | 3 | both_compile | 5.592 | 3.890 |  |
| ndt_files.txt | 2 | rfort_failure | 7.241 | 6.719 | error: reduction extents and section bounds must be constant integers: `n` is not a previously defined integer PARAMETER |
| qua_files.txt | 7 | rfort_timeout | 90.350 | 22.408 |  |
| rdv_files.txt | 8 | rfort_failure | 7.927 | 15.670 | error: PARAMETER initializer must be a supported integer constant expression |
| rou_files.txt | 2 | both_compile | 0.771 | 1.566 |  |
| sfn_files.txt | 3 | both_compile | 20.609 | 16.092 |  |
| smb_files.txt | 9 | rfort_failure | 11.469 | 7.084 | error: constructor implied-DO index `j` requires a locally declared scalar INTEGER variable |
| vld_files.txt | 2 | both_compile | 3.113 | 2.128 |  |
| wcn_files.txt | 4 | both_compile | 6.217 | 3.410 |  |
| wdf_files.txt | 11 | rfort_failure | 5.762 | 24.257 | Error: ALLOCATABLE or POINTER array component `xx` in recursive type `data_series` is not yet supported |

Counts: {'rfort_failure': 8, 'both_compile': 9, 'rfort_timeout': 1}

Full diagnostics and commands are in report.json and each case directory. A compilation failure is not automatically a compiler bug; inspect the reference result and check the source and required flags. Compilation success does not establish runtime correctness.

## Failure details

- **bsg_files.txt**, `bs.f90:435`: `recursive specialized subroutine calls are
  not yet supported`. The statement calls `bs_greek_one` recursively twice to
  combine call and put results. The source declares a recursive REAL-valued
  function with CHARACTER inputs and an optional REAL input. The diagnostic
  calls this a subroutine; the underlying specialization issue needs reduction.
- **cde_files.txt** (`date_mdy.f90`, `xcheck_dates_equal.f90`):
  `multidimensional array @record$dates$month requires a statically known shape`,
  caused by `ndates is not a previously defined integer PARAMETER`.
  No source location is provided. The driver explicitly declares
  `integer, parameter :: nsym=3, ndates=2` and
  `type(month_year) :: dates(ndates,nsym)`; the called `check_dates_equal`
  procedure takes `dates(:,:)`. This does not establish a missing PARAMETER
  declaration in the user's source; scope/shape handling needs investigation.
- **clt_files.txt** and **rdv_files.txt**, `statistics.f90:1082`:
  `prob = bin_freq(xthresh,xx)/real(max(1,size(xx)),kind=dp)` receives
  `PARAMETER initializer must be a supported integer constant expression`.
  This repeats the location and diagnostic from the first sample's
  `rcsv_files.txt` failure. It is an executable assignment, not a PARAMETER
  initializer. A shared underlying cause has not yet been isolated.
- **ffd_files.txt**, `reg_stats.f90:188`:
  `call solve_lsq_stats(aa(:,iuse),bb,xx_use,ierr,tstat=tstat_use)` fails generic
  resolution. rfort reports argument ranks/types as a rank-two DOUBLE PRECISION
  array, two rank-one DOUBLE PRECISION arrays, a scalar INTEGER(4), and a
  rank-one DOUBLE PRECISION array. `iuse` is an integer vector subscript, and
  `tstat` is supplied by keyword. Further reduction is needed to identify which
  part of interface matching fails; the full group builds with gfortran.
- **ndt_files.txt**, `xnext_date.f90:7`, `type(date_mdy) :: date(n)`:
  `reduction extents and section bounds must be constant integers: n is not a
  previously defined integer PARAMETER`. The driver declares
  `integer, parameter :: n=400` at line 5. It subsequently subtracts date array
  sections with an overloaded operator and reduces the integer differences.
  The error does not prove those bounds are nonconstant; it needs reduction.
- **smb_files.txt**, `gnuplot.f90:499`: rejects declared INTEGER implied-DO
  index `j` inside a constructor nested in SUM within FORALL. This repeats the
  corrected `reg_files.txt` failure. The verified standalone repro is
  [xforall_constructor_implied_do.f90](../xforall_constructor_implied_do.f90).
- **wdf_files.txt**: `ALLOCATABLE or POINTER array component xx in recursive
  type data_series is not yet supported`, with no source location.
  `data_series.f90:27-33` defines the type, including REAL allocatable `xx(:)`
  and LOGICAL allocatable `good(:)`, CHARACTER fields, and no self-referential
  derived-type components. Calling the type recursive is misleading; the exact
  triggering lowering path has not been isolated.

## Compilation timeout

**qua_files.txt**, seven sources ending in `xquantile.f90`, reaches rfort's
90-second limit with no diagnostic output. gfortran builds it in 22.408 seconds.
The result is a timeout, not an established semantic rejection or proof of an
infinite loop. Repeating with a larger limit and compiler timing/profiling would
distinguish a slow compilation from a nonterminating path.

## Saved evidence and reproduction

The snapshot, copied sources, source hashes, complete commands, stdout/stderr,
and per-compiler results are saved locally under
`target/2015-compat/sample2-20261103/`. Its `report.json` is machine-readable;
`report.md` is the original generated table. Generated artifacts are ignored
by Git; this evaluation document preserves the findings in the repository.

To repeat using the current prebuilt release executable, from Windows CMD:

```cmd
python tests\run_source_list_compat.py --root C:\fortran\2015 --lists ^
  bsg_files.txt cde_files.txt clt_files.txt ffa_files.txt ffd_files.txt flu_files.txt ^
  knd_files.txt mma_files.txt mop_files.txt ndt_files.txt qua_files.txt rdv_files.txt ^
  rou_files.txt sfn_files.txt smb_files.txt vld_files.txt wcn_files.txt wdf_files.txt ^
  --limit 18 --compile-timeout 90
```

This creates a new report directory; it neither rebuilds the compiler nor runs
generated programs. Use `--compiler` to select another prebuilt executable.

## Suggested follow-up

The repeated `statistics.f90:1082` failure affects several independent drivers
and is a useful first target for reduction. The date-array failures also merit
small repros because both incorrectly claim declared parameters are missing.
The gnuplot failure already has a verified repro. Separately, profile the
quantile compilation timeout; keep performance findings distinct from semantic
failures.

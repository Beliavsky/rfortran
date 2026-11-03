# Fortran 2015 fourth source-list sample: 2026-11-03

Sources: `C:\fortran\2015`

This targeted sample adds 24 groups, bringing the four samples to 82 distinct
source lists. Twelve build with both compilers, four fail only with rfort, and
eight fail with both. No compilation timed out; there were no missing INCLUDE
files. This is compilation coverage, not a runtime correctness assessment or
an estimate of overall compiler compatibility.

To reduce repeated blockers, selection excluded lists containing `gnuplot.f90`,
`data_series.f90`, `moving_average.f90`, `statistics.f90`, `util.f90`, `bs.f90`,
`lsq_miller.f90`, `m_time.f90`, `iso_varying_string.f90`, or `uncon_min.f90`.
It also excluded the 58 previously tested lists. Candidates had at most 15
listed sources, all present, and at most 400,000 total source bytes. From each
of three buckets (one or two sources, three to five, six to fifteen), eight
lists were chosen greedily to favor previously unseen dependency source names,
then unseen source names overall, then smaller total size. File presence does
not establish that a list supplies all module dependencies.

The resulting 120 listed source entries contain 95 unique source names, 84
not present in the first three samples. The selection metadata is saved locally
in `target/2015-compat/fourth-sample-selection.json`.

Compiler: existing rfort 0.1.0-alpha.1, Windows x86-64. Reference:
GNU Fortran (GCC) 17.0.0 20260510 (experimental). Each compiler invocation has
a 90-second limit. No compiler files or original external sources were changed.

Compilation and linking only; no generated program was executed.
Source-list order is preserved. rfort uses its default optimization; gfortran uses -O0 -ffree-line-length-none. Each compiler has an isolated workspace.
The rfort executable was snapshotted; the source tree and compiler code were not modified.

rfort SHA256: `a89f046e71e2f63838bf988d04dabcba2f92bb2103b88df0612f712bbbcc76a0`

| Source list | Files | Result | rfort seconds | gfortran seconds | First rfort diagnostic |
|---|---:|---|---:|---:|---|
| ach_files.txt | 2 | both_compile | 0.467 | 0.868 |  |
| cbl_files.txt | 5 | both_failed | 0.141 | 0.882 | error: required by module `cobyla2_mod`: module interface `calcfc_mod.rmod` not found; compile its module first or supply -I <directory> |
| cun_files.txt | 3 | rfort_failure | 0.160 | 1.230 | error: reduction extents and section bounds must be constant integers: `n` is not a previously defined integer PARAMETER |
| cxb_files.txt | 4 | both_compile | 53.187 | 2.388 |  |
| fit_files.txt | 7 | both_failed | 0.238 | 2.369 | error: required by module `gammq_all_mod`: module interface `nr.rmod` not found; compile its module first or supply -I <directory> |
| flt_files.txt | 3 | both_failed | 0.186 | 1.482 | error: undefined function or subroutine `pxflocaltime` |
| fmp_files.txt | 5 | both_compile | 1.118 | 2.686 |  |
| fud_files.txt | 2 | both_compile | 0.908 | 1.726 |  |
| gfortran_opr_files.txt | 11 | both_failed | 1.157 | 6.132 | Error: `mean_abs_diff` is not a public entity of module `statistics_mod` in USE ONLY |
| icr_files.txt | 2 | both_compile | 0.671 | 1.406 |  |
| ipr_files.txt | 14 | both_failed | 1.069 | 8.538 | error: required by module `read_mdy_alloc_mod`: module interface `util_mod.rmod` not found; compile its module first or supply -I <directory> |
| itf_files.txt | 2 | both_compile | 0.571 | 1.235 |  |
| nmo_files.txt | 6 | both_failed | 0.121 | 7.050 | Error: stream did not contain valid UTF-8 |
| npo_files.txt | 5 | both_compile | 1.539 | 3.416 |  |
| nwu_files.txt | 6 | both_failed | 0.398 | 2.559 | error: unexpected '(': expected one of: kw_type, kw_class, kw_typeof, kw_classof, kw_interface, attr_stmt, proc_decl, bare_end, kw_error, kw_allocate, kw_deallocate, kw_nullify, keyword end, kw_enumeration, kw_enum, kw_forall, kw_forall_block, kw_parameter, kw_where, kw_where_block, kw_public, kw_private, kw_return, keyword implicit, keyword print, kw_read, kw_read_empty, file_control, file_position, keyword write, keyword if, kw_do, construct_name, named_exit, named_cycle, keyword integer, keyword real, kw_complex, kw_dimension, kw_save_all, keyword double, keyword character, keyword logical, identifier, label, format_spec, kw_data, kw_equivalence, kw_common, entry_stmt, kw_goto, kw_continue, ',', kw_call, kw_contains, kw_use, kw_select, kw_block, kw_associate, kw_exit, kw_cycle, kw_stop, keyword stop |
| opn_files.txt | 2 | rfort_failure | 0.130 | 1.167 | error: derived-valued expression `ivec` requires a derived assignment or argument |
| pma_files.txt | 10 | both_failed | 0.661 | 6.780 | error: required by module `monthly_returns_mod`: module interface `first_last_true_mod.rmod` not found; compile its module first or supply -I <directory> |
| qpt_files.txt | 9 | both_compile | 2.008 | 4.201 |  |
| rgm_files.txt | 5 | both_compile | 1.809 | 2.177 |  |
| sev_files.txt | 4 | rfort_failure | 0.353 | 1.968 | error: FORALL destination subscripts must be known for every iteration: `i` is not a previously defined integer PARAMETER |
| swb_files.txt | 2 | both_compile | 0.589 | 0.909 |  |
| tpp_files.txt | 2 | both_compile | 0.652 | 1.225 |  |
| twi_files.txt | 2 | both_compile | 0.545 | 1.159 |  |
| xmd_files.txt | 7 | rfort_failure | 3.709 | 10.497 | error: reduction extents and section bounds must be constant integers: `nget` is not a previously defined integer PARAMETER |

Counts: {'both_compile': 12, 'both_failed': 8, 'rfort_failure': 4}

Full diagnostics and commands are in report.json and each case directory. A compilation failure is not automatically a compiler bug; inspect the reference result and check the source and required flags. Compilation success does not establish runtime correctness.

## rfort-only failures

- **cun_files.txt**, `coupon.f90:17`: `collected = .false.` inside impure
  elemental function `avg_tries` is rejected with `reduction extents and section
  bounds must be constant integers: n is not a previously defined integer
  PARAMETER`. The function declares `integer, intent(in) :: n, k` and the
  automatic local array `logical :: collected(n)`. This exposes runtime-sized
  LOGICAL whole-array assignment in an elemental procedure; a small repro is
  needed to distinguish which part of the context triggers the error.
- **opn_files.txt**, `xoptional.f90:7`: `print *, ivec()` is rejected with
  `derived-valued expression ivec requires a derived assignment or argument`.
  The module defines type `ivec` with three default-initialized INTEGER
  components. Its empty structure constructor is supplied directly to
  list-directed output. The error points to the PRINT, not a missing component
  initializer. This two-file group is already very small.
- **sev_files.txt**, `xstdev.f90:23`:
  `forall (imethod=1:nmethods) xsd(i,imethod) = sd(x(i1:i2),imethod)` is rejected
  because FORALL destination subscripts supposedly must be known for every
  iteration, and `i` is not an integer PARAMETER. Here `i` is a surrounding
  DO-loop index; `imethod` is the FORALL index. This differs from the earlier
  gnuplot constructor implied-DO diagnostic and merits its own repro.
- **xmd_files.txt**, `xmonthly_data.f90:23`:
  `dates_get = next(dates_get)` fails with `reduction extents and section bounds
  must be constant integers: nget is not a previously defined integer
  PARAMETER`. The driver explicitly declares `integer, parameter :: nget=3`
  as part of line 5, and `dates_get(nget)` has derived type `month_year`.
  This resembles the earlier date-array scope/lowering failures but is a new
  failing statement; a shared underlying cause has not been established.

## Groups rejected by both compilers

These outcomes do not establish rfort-only full-program failures. Compiler
limitations found independently in them are noted separately.

- **cbl_files.txt**: `cobyla.f90` defines module `cobyla2_mod` and uses
  `calcfc_mod`, but the list supplies `calcfc.f90`, an external subroutine,
  rather than the needed module source. The driver also requests `cobyla2`
  rather than `cobyla2_mod`. The alternative **cxb_files.txt**, using
  `calcfc_mod.f90`, `cobyla2.f90` and `xxcobyla_orig.f90`, builds with both.
- **fit_files.txt**: `gammq_all.f90:49` uses module `nr`, absent from this clean
  source group. That prevents the downstream simple-regression module and
  driver from building with gfortran as well as rfort.
- **flt_files.txt**: rfort rejects undefined `pxflocaltime`; gfortran reaches
  linking but cannot resolve `fstat_` and `pxflocaltime_`. These file-time
  routines need external/platform-specific support not supplied by this group.
- **gfortran_opr_files.txt**: `optimal_positions.f90:4` requests
  `mean_abs_diff` from `statistics_mod`, but the supplied `gf_statistics.f90`
  module does not provide that public entity. Both compilers reject it.
- **ipr_files.txt**: `read_mdy_alloc.f90:11` requires `util_mod`, not supplied
  by the list; gfortran also reports missing `statistics_mod` when compiling
  `write_table_dates.f90`. A complete dependency list is needed first.
- **nmo_files.txt**: gfortran reports missing `util_mod` in
  `distributions.f90:6`. rfort stops earlier with `stream did not contain valid
  UTF-8`, without identifying the source path. A direct decoding check finds
  invalid byte `0x92` at byte offset 8517 in `distributions.f90`. No source
  encoding conversion was performed. Encoding handling and diagnostic location
  are separate from the incomplete module dependencies.
- **nwu_files.txt**: gfortran rejects callback interface INTENT mismatches
  for dummy procedure `calfun` in `newuoa_test.f90:22,51`. Independently, rfort
  rejects the typed implicit declaration `implicit real (wp) (a-h, o-z)` at
  `newuoa.f90:140`. The parsing case is worth reducing, but the supplied full
  group first needs valid callback interfaces for a fair executable comparison.
- **pma_files.txt**: `monthly_returns_alloc.f90:8` requires missing
  `first_last_true_mod`, and `print_monthly_returns_alloc.f90:4` requires
  missing `print_corr_mat_stats_mod`. Both compilers stop before the driver
  can be built from this source list.

## Successful compilations

Both compilers build `ach_files.txt`, `cxb_files.txt`, `fmp_files.txt`,
`fud_files.txt`, `icr_files.txt`, `itf_files.txt`, `npo_files.txt`,
`qpt_files.txt`, `rgm_files.txt`, `swb_files.txt`, `tpp_files.txt`, and
`twi_files.txt`. These include CHARACTER operators, a COBYLA optimization
driver, missing-data filling, function-based input, explicit interfaces,
position normalization, quadratic programming, regime simulation, and derived
types with pointer components. Compilation alone does not validate execution
or the correctness of the supplied source programs.

## Saved evidence and reproduction

The binary snapshot, copied sources and hashes, commands, compiler output and
machine-readable results are saved locally under
`target/2015-compat/sample4-20261103/`, including `report.json` and `report.md`.
Generated artifacts are ignored by Git; this document preserves the findings.

Repeat the selected groups from Windows CMD:

```cmd
python tests\run_source_list_compat.py --root C:\fortran\2015 --lists ^
  ach_files.txt cbl_files.txt cun_files.txt cxb_files.txt fit_files.txt flt_files.txt ^
  fmp_files.txt fud_files.txt gfortran_opr_files.txt icr_files.txt ipr_files.txt ^
  itf_files.txt nmo_files.txt npo_files.txt nwu_files.txt opn_files.txt pma_files.txt ^
  qpt_files.txt rgm_files.txt sev_files.txt swb_files.txt tpp_files.txt twi_files.txt ^
  xmd_files.txt --limit 24 --compile-timeout 90
```

The harness creates a new report directory and snapshots the current prebuilt
release executable. It does not rebuild the compiler or run generated programs.

## Suggested next work

Reduce the four rfort-only failures before another broad sample. The derived
constructor output case is already small; the coupon function provides a
focused LOGICAL automatic-array case. The FORALL destination case tests a
different context from the existing constructor repro. Separately, reduce
typed IMPLICIT parsing without carrying over the invalid NEWUOA callback
interfaces. The eight reference failures show why further source-list testing
should validate dependency completeness rather than just file presence.

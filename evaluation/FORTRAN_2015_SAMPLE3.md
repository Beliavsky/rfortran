# Fortran 2015 third source-list sample: 2026-11-03

Sources: `C:\fortran\2015`

This sample adds 18 source-list groups not included in the
[first](FORTRAN_2015_SAMPLE.md) or [second](FORTRAN_2015_SAMPLE2.md) sample.
Five build with both compilers, nine fail only with rfort, and four fail with
both. No compilation timed out and no INCLUDE file was missing.

Selection took six evenly spaced names from each of three alphabetically
sorted buckets of previously untested source lists whose listed files all
exist: one or two sources, three to seven, and eight to twenty. This checks
file presence, not completeness of module dependencies or validity of the
program. There are 109 source entries across the groups, including shared
dependencies. This is a coverage sample, not an estimate of overall compatibility.

Compiler: existing rfort 0.1.0-alpha.1, Windows x86-64. Reference:
GNU Fortran (GCC) 17.0.0 20260510 (experimental). Each compilation has a
90-second limit. The snapshot was not rebuilt, and compiler files and originals
under `C:\fortran\2015` were unchanged. Two additional diagnostic probes modify
only disposable copied drivers, as described below.

Compilation and linking only; no generated program was executed.
Source-list order is preserved. rfort uses its default optimization; gfortran uses -O0 -ffree-line-length-none. Each compiler has an isolated workspace.
The rfort executable was snapshotted; the source tree and compiler code were not modified.

rfort SHA256: `a89f046e71e2f63838bf988d04dabcba2f92bb2103b88df0612f712bbbcc76a0`

| Source list | Files | Result | rfort seconds | gfortran seconds | First rfort diagnostic |
|---|---:|---|---:|---:|---|
| bcx_files.txt | 5 | both_compile | 27.524 | 14.556 |  |
| bnr_files.txt | 2 | both_compile | 0.628 | 1.283 |  |
| cdf_files.txt | 17 | rfort_failure | 12.140 | 32.444 | Error: ALLOCATABLE or POINTER array component `xx` in recursive type `data_series` is not yet supported |
| egn_files.txt | 11 | rfort_failure | 22.223 | 21.857 | error: constructor implied-DO index `j` requires a locally declared scalar INTEGER variable |
| elm_files.txt | 3 | both_compile | 13.345 | 17.240 |  |
| eph_files.txt | 2 | both_failed | 0.244 | 1.678 | error: unexpected keyword end: expected one of: identifier |
| ivs_files.txt | 2 | both_failed | 0.162 | 1.030 | error: READ currently supports one status or branch control |
| mavg_files.txt | 3 | rfort_failure | 0.273 | 1.433 | error: recursive CHARACTER function `moving_average_vec` is not yet supported |
| mco_files.txt | 11 | both_failed | 0.892 | 14.566 | error: required by module `read_matrix_alloc_mod`: module interface `util_mod.rmod` not found; compile its module first or supply -I <directory> |
| mtf_files.txt | 2 | both_compile | 0.987 | 1.405 |  |
| pse_files.txt | 3 | both_compile | 12.622 | 9.817 |  |
| ray_files.txt | 12 | rfort_failure | 14.781 | 11.985 | error: PARAMETER initializer must be a supported integer constant expression |
| rgn_files.txt | 1 | rfort_failure | 0.330 | 0.638 | Error: Windows linking failed; install Visual Studio C++ Build Tools and the Windows SDK, or set RFORT_LIB_PATH to their library directories |
| sbd_files.txt | 6 | rfort_failure | 7.233 | 9.518 | error: constructor implied-DO index `j` requires a locally declared scalar INTEGER variable |
| scf_files.txt | 10 | both_failed | 0.945 | 9.900 | error: required by module `read_matrix_alloc_mod`: module interface `util_mod.rmod` not found; compile its module first or supply -I <directory> |
| ucm_files.txt | 2 | rfort_failure | 0.357 | 1.885 | error: unexpected identifier to: expected one of: percent, arrow, '=', '(' |
| upd_files.txt | 4 | rfort_failure | 0.651 | 1.968 | Error: invalid bounds for array `rms` |
| vrr_files.txt | 13 | rfort_failure | 29.211 | 30.906 | error: constructor implied-DO index `j` requires a locally declared scalar INTEGER variable |

Counts: {'both_compile': 5, 'rfort_failure': 9, 'both_failed': 4}

Full diagnostics and commands are in report.json and each case directory. A compilation failure is not automatically a compiler bug; inspect the reference result and check the source and required flags. Compilation success does not establish runtime correctness.

## Programs accepted by both compilers

- `bcx_files.txt`: `xbox_cox.f90`, Box-Cox transformations.
- `bnr_files.txt`: `xbinary.f90`, binary representations and LOGICAL operations.
- `elm_files.txt`: `xelem_char.f90`, CHARACTER element selection and defaults.
- `mtf_files.txt`: `xmatch_tf.f90`, logical matching.
- `pse_files.txt`: `xprint_random_seeds.f90`, random-seed utilities.

## rfort-only failures

- **cdf_files.txt**, ending in `xcombine_date_frames.f90`: rejects the
  allocatable `xx(:)` component in `data_series`, saying the type is recursive.
  This repeats the second sample's `wdf_files.txt` failure. The declaration at
  `data_series.f90:27-33` has no self-referential derived-type component, so
  that description is misleading. No source location accompanies the error.
- **egn_files.txt**, **sbd_files.txt**, and **vrr_files.txt**, ending in
  `xreg_nn_1d.f90`, `xsine_bound.f90`, and `xvar_ratio.f90`, respectively:
  all repeat the declared implied-DO index rejection at `gnuplot.f90:499`.
  The verified repro is
  [xforall_constructor_implied_do.f90](../xforall_constructor_implied_do.f90).
- **mavg_files.txt**, `moving_average.f90:68`:
  `xma = moving_average_vec(xx,n)` is rejected as an unsupported recursive
  CHARACTER function. The procedure is explicitly `recursive pure`, with
  REAL(kind=dp) input and REAL(kind=dp) result `xma(size(xx))`. It is not a
  CHARACTER-valued function. This resembles the earlier recursive numeric
  array-result reduction; a common root cause has not been proven.
- **ray_files.txt**, `util.f90:8070`:
  `xma = moving_average_xlen(xx,real(n,kind=dp))` receives
  `PARAMETER initializer must be a supported integer constant expression`.
  This repeats the first sample's `fve_files.txt` failure at the same statement.
- **rgn_files.txt**, `xrgnuplot.f90`: the three-line program calls
  `system("gnu sin.gnu")`. rfort gets as far as linking, then LLD reports
  `undefined symbol: rfort$system`. The outer diagnostic recommends installing
  Windows build tools, but the concrete failure is an undefined symbol.
  SYSTEM is a compiler extension accepted by gfortran here; this result is not
  evidence of missing standard-conforming EXECUTE_COMMAND_LINE support.
  No generated program was run, so no gnuplot process was launched.
- **ucm_files.txt**, `uncon_min.f90:1557`: rejects the legacy computed-GOTO
  statement `if (lh >= nn1o2) go to (210,210,210,210,210,210,160,120,160,10,10,20), i`
  at `to`. gfortran compiles the full group. The exact parsing interaction needs
  a smaller repro.
- **upd_files.txt**, ending in `xupdate.f90`: `invalid bounds for array rms`,
  caused by a requirement for constant explicit array bounds and the claim
  that `in` is not an integer PARAMETER. The relevant declaration in
  `lsq_miller.f90:560` is `real(dp) :: rms(in+1:ncol), ..., work(in+1:ncol)`
  inside `partial_corr`; `in` is an INTEGER INTENT(IN) dummy declared at line
  553. This is a local automatic-array declaration with a nonunit lower bound.
  The diagnostic itself supplies no source location. gfortran builds the group.

## Groups rejected by both compilers

- **eph_files.txt**: the driver `xepoch_time.f90:3` says
  `use m_time, only: unix_to_date`, while `m_time.f90:1` defines
  `module M_time_mod`. gfortran cannot find `m_time.mod`. Independently, rfort
  rejects the dummy argument named `end` in
  `subroutine substitute(targetline,old,new,ierr,start,end)` at
  `m_time.f90:3731`. The supplied full group has a module-name mismatch.
- **ivs_files.txt**: gfortran rejects `call put(cc)` at
  `xiso_varying_string.f90:15`: the supplied varying-string module has no
  matching array PUT specific. rfort stops earlier with
  `READ currently supports one status or branch control`. Its source location
  points to line 1 of a generated combined file, not the original READ.
- **mco_files.txt** and **scf_files.txt**, ending in `xxma_combo.f90` and
  `xsma_combo_fit.f90`: both omit the source defining `util_mod`.
  `read_matrix_alloc.f90:3` uses that module; neither clean build can resolve
  it. Add its source and any necessary dependencies in order before evaluating
  full-program compiler compatibility. The original lists were not modified.

## Diagnostic probes using corrected copies

Two probes separate source mismatches from compiler limitations. They are
additional to the 18 original groups; their results do not change the original
table's classifications.

1. **epoch_module_name** changes only `use m_time` to `use m_time_mod` in a
   copied driver. gfortran builds successfully. rfort still rejects the dummy
   named `end` at `m_time.f90:3731`.
2. **varying_string_scalar_put** removes only the unsupported `call put(cc)`
   from a copied driver, leaving scalar `call put(cc(1))` and the remaining
   varying-string operations. gfortran builds successfully. rfort still emits
   the READ-control diagnostic with an unhelpful generated-file location.
   The exact READ and control combination still needs reduction.

Neither probe executable was run. The copied inputs, logs, commands and results
are saved under `target/2015-compat/sample3-20261103/corrected-probes/`, including
a machine-readable `report.json`.

## Saved evidence and reproduction

The compiler snapshot, copied sources, source hashes, commands, full diagnostics
and original-group results are saved locally under
`target/2015-compat/sample3-20261103/`, with `report.json` and `report.md`.
Generated artifacts are ignored by Git; this document preserves the findings.

Repeat the original 18 groups from Windows CMD with:

```cmd
python tests\run_source_list_compat.py --root C:\fortran\2015 --lists ^
  bcx_files.txt bnr_files.txt cdf_files.txt egn_files.txt elm_files.txt eph_files.txt ^
  ivs_files.txt mavg_files.txt mco_files.txt mtf_files.txt pse_files.txt ray_files.txt ^
  rgn_files.txt sbd_files.txt scf_files.txt ucm_files.txt upd_files.txt vrr_files.txt ^
  --limit 18 --compile-timeout 90
```

The runner creates a new report directory and snapshots the current prebuilt
release executable. It does not rebuild rfort or run generated programs.

## Suggested follow-up

[Six verified standalone cases](FORTRAN_2015_REPROS.md) now cover the new
failure patterns: automatic-array bounds, the dummy named `end`, computed
GO TO, ADVANCE with IOSTAT, the SYSTEM extension, and recursive numeric array
results. The bounds reduction changes the diagnostic; the other five preserve
the reported diagnostic or undefined symbol. The repeated gnuplot failure
already has a verified standalone repro. Resolve the missing `util_mod`
dependencies before counting the two incomplete source lists as compiler failures.

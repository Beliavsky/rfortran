# Standalone repros from the Fortran 2015 samples

Checked on 2026-11-03 using an existing Windows x86-64 rfort
0.1.0-alpha.1 executable, SHA256
`a89f046e71e2f63838bf988d04dabcba2f92bb2103b88df0612f712bbbcc76a0`.
No compiler source was changed or rebuilt. GNU Fortran (GCC) 17.0.0
20260510 (experimental) compiles all six cases below. Its executables were
run for the five standard-language cases and produced the expected output.
The SYSTEM extension case was compiled and linked only.

These cases reduce findings in the
[third sample](FORTRAN_2015_SAMPLE3.md). The automatic-array case preserves a
valid failing construct but changes the diagnostic; it is not an exact
reproduction of the original error message. The other five reproduce the
corresponding diagnostic or unresolved symbol.

| Source | Origin | Observed rfort failure | Expected reference behavior |
|---|---|---|---|
| [xautomatic_array_lower_bound.f90](../xautomatic_array_lower_bound.f90) | `upd_files.txt`, `lsq_miller.f90:560` | `undefined function or subroutine rms` at a valid array declaration | Bounds 2 and 4; values 2, 3, 4 |
| [xdummy_named_end.f90](../xdummy_named_end.f90) | `eph_files.txt`, `m_time.f90:3731` | Rejects `end` as a dummy argument name | Prints 3 |
| [xcomputed_goto_if.f90](../xcomputed_goto_if.f90) | `ucm_files.txt`, `uncon_min.f90:1557` | Rejects `to` in single-line IF containing computed GO TO | Prints 20 |
| [xread_advance_iostat.f90](../xread_advance_iostat.f90) | `ivs_files.txt`, `iso_varying_string.f90` | `READ currently supports one status or branch control` | Reads A, reports IOSTAT 0 |
| [xsystem_extension.f90](../xsystem_extension.f90) | `rgn_files.txt`, `xrgnuplot.f90` | LLD: `undefined symbol: rfort$system` | gfortran accepts the SYSTEM extension |
| [xrecursive_real_array_result.f90](../xrecursive_real_array_result.f90) | `mavg_files.txt`, `moving_average.f90:68` | Calls a REAL result an unsupported recursive CHARACTER function | Prints 1, 2, 3 |

## Automatic-array distinction

The local array has bounds `in+1:ncol`, with `in` an integer dummy argument
and `ncol` a module variable assigned before the call. Both are available on
procedure entry. In this reduction, rfort misidentifies the array declaration
as an undefined procedure reference. In the original larger program it instead
reports `invalid bounds for array rms`, followed by a constant-expression
requirement for `in`.

A control variant with both `in` and `ncol` passed as dummy arguments compiles
with rfort. Thus nonunit lower bounds alone do not reproduce the failure.
Another failing probe uses `rms(ncol)`, suggesting the module-variable bound
matters even without a nonunit lower bound. These probes identify a narrower
area to investigate; they do not establish a single shared underlying cause
for all the original program's bounds handling.

## READ-control distinction

The reduced READ uses `ADVANCE='NO'` together with `IOSTAT=ios`. That combination
alone triggers the diagnostic; no END, EOR or ERR branch control is needed.
The repro creates a formatted scratch file and supplies its own input, so
running the reference program does not require keyboard input or a fixture.

## SYSTEM extension

`CALL SYSTEM` is an extension, not a standard Fortran intrinsic. Its rejection
does not establish a problem with standard `EXECUTE_COMMAND_LINE`. The repro
replaces the original external gnuplot command with harmless `echo system repro`.
The outer rfort message recommends installing Windows build tools, but the
concrete linker diagnostic is an undefined symbol.

## Reproduce and saved evidence

From Windows CMD, for example:

```cmd
target\release\rfort.exe xread_advance_iostat.f90
gfortran xread_advance_iostat.f90 -o target\read_advance_reference.exe
target\read_advance_reference.exe
```

Substitute any of the other source names to check it. Commands use the existing
release binary directly and do not rebuild the compiler.

The checked binary snapshot, full compiler diagnostics, commands and reference
execution results are saved locally under
`target/2015-compat/standalone-repros-20261103/`, including `report.json`.
Exploratory bounds variants are under its `bounds-probes/` subdirectory.
Those generated artifacts are ignored by Git; the six source files and this
document preserve the repros in the repository.

The earlier verified gnuplot FORALL/constructor repro is
[xforall_constructor_implied_do.f90](../xforall_constructor_implied_do.f90).

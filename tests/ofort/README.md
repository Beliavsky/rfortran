# ofort compatibility sample

Run the selected external fixtures without building or modifying ofort:

```bat
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --report target/ofort-compat.json
```

The runner defaults to target/release/rfort.exe on Windows (rfort elsewhere).
Use --compiler to select a different build. gfortran must be on PATH, or pass
--reference with its path. No ofort Python test hooks or executables are used.
Each reference and rfort executable runs in a separate temporary directory with
closed standard input and a timeout (15 seconds by default; --timeout changes
it). Both native O0 and O2 are checked. Source files stay in the external tree;
compiler outputs, module files and program-created files go in temporary work
directories. This initial selection consists of standalone, deterministic
programs that do not need input files or companion sources.

selection.json selects 18 cases covering loops, arrays, numeric semantics,
CHARACTER, optional arguments, WHERE, and array intrinsics. This is a development
sample, not a pass-rate estimate for all ofort tests or Fortran conformance.
Expected behavior comes from compiling and running the same source with
gfortran, rather than adopting interpreter-specific diagnostics or extensions.

Reports distinguish compile errors, compiler crashes, runtime errors/crashes,
timeouts, unexpected runtime stderr, and wrong output. A case passes only when
both O0 and O2 pass. Failed reference compilation/execution and missing files
are recorded separately. Nonzero exit status from the runner is opt-in via
--strict, so an incomplete baseline can be collected without aborting a work
script. Missing compiler tools are always a command-line error.

Comparison tokenizes whitespace and accepts T/F or .TRUE./.FALSE. Integer tokens
are compared exactly. Real tokens allow relative error 2e-6 or absolute error
1e-7, reflecting the compiler's current list-directed precision. This comparison
is unsuitable for format-layout tests; those belong in the ordinary regression
suite. Reports retain unnormalized stdout/stderr, source SHA-256 hashes, compiler
binary hash/path, and reference compiler version for investigation.

## Recorded results (Windows x64, 2026-10-10)

- baseline.json: 3/18 cases pass at both O0/O2; 15 compile failures.
- after-where.json: 5/18 pass; 13 compile failures. xwhere_basic and
  xwhere_elsewhere_forms now pass. All 18 reference programs build and run.
- after-explicit-bounds.json: still 5/18 pass. Four cases now parse their explicit
  lower-bound declarations and reach later unsupported features: nested/rank-two
  WHERE and array-intrinsic keyword arguments. The local 89_explicit_bounds.f90
  regression tests the new behavior independently against gfortran.

The remaining WHERE probes exercise nested/multidimensional masking, constructors
inside expressions, and CSHIFT, which need further work. The independent local
88_where.f90 regression already checks mask capture, active-only RHS evaluation,
ELEMENTAL calls, overlapping assignments, ELSEWHERE precedence, LOGICAL arrays,
zero extents, and reduction RHS behavior.

Use python tests/test_where.py for the local interpreter/native regression and
python tests/test_ofort_compat_runner.py for the report/comparison checks. The
local tests do not require the external ofort checkout.

The local `90_masked_reductions.f90` fixture extracts numeric reductions into a
supported subset: reordered ARRAY/DIM/MASK keywords, rank-one DIM=1, scalar and
array masks, sections and empty arrays. `python tests/test_masked_reductions.py`
checks interpreter/native O0/O2 output against gfortran. The external sample is
still 5/18: the full reductions probe requires additional features, including
multidimensional DIM results and other array operations.

`91_logical_reductions.f90` covers COUNT (including KIND=8), ANY/ALL keywords,
rank-one DIM=1, empty inputs, sections, relational masks and pure functions with
local arrays. Run `python tests/test_logical_reductions.py` independently of ofort.

The former pending function-array SUM reproduction now passes and is included in
`92_function_arrays.f90`. `python tests/test_function_arrays.py` tests fixed-size
read-only array arguments to PURE functions in the interpreter and native O0/O2,
with gfortran comparison. This subset copies all input elements; actual arrays
must be fully initialized.

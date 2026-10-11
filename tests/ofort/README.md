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

At that checkpoint, the remaining WHERE probes required nested/multidimensional
masking, constructors inside expressions, and CSHIFT. These now pass. The local
88_where.f90 regression already checks mask capture, active-only RHS evaluation,
ELEMENTAL calls, overlapping assignments, ELSEWHERE precedence, LOGICAL arrays,
zero extents, and reduction RHS behavior.

Use python tests/test_where.py for the local interpreter/native regression and
python tests/test_ofort_compat_runner.py for the report/comparison checks. The
local tests do not require the external ofort checkout.

The local `90_masked_reductions.f90` fixture extracts numeric reductions into a
supported subset: reordered ARRAY/DIM/MASK keywords, rank-one DIM=1, scalar and
array masks, sections and empty arrays. `python tests/test_masked_reductions.py`
checks interpreter/native O0/O2 output against gfortran. At that checkpoint the external sample was
5/18; the reductions probe now passes with the constructor/RESHAPE work below.

`91_logical_reductions.f90` covers COUNT (including KIND=8), ANY/ALL keywords,
rank-one DIM=1, empty inputs, sections, relational masks and pure functions with
local arrays. Run `python tests/test_logical_reductions.py` independently of ofort.

The former pending function-array SUM reproduction now passes and is included in
`92_function_arrays.f90`. `python tests/test_function_arrays.py` tests fixed-size
read-only array arguments to PURE functions in the interpreter and native O0/O2,
with gfortran comparison. This subset copies all input elements; actual arrays
must be fully initialized.

`93_integer_huge.f90` covers integer HUGE for kinds 1/2/4/8, preserved KIND,
constant initializers and empty-array inquiries. The full external reductions
probe now gets past integer HUGE and zero-extent/LOGICAL matrix declarations.
Multidimensional DIM reductions are covered by checkpoint 34 below.
`94_empty_logical_matrices.f90` and `python tests/test_empty_matrices.py` cover
empty reductions, bounds, logical sections and invalid empty-dimension access.

`95_dim_reductions.f90` and `python tests/test_dim_reductions.py` cover constant
DIM for SUM/PRODUCT/MINVAL/MAXVAL/COUNT/ANY/ALL, with masks, empty axes,
nested reductions and rank-three sections.

`96_reshape_constructors.f90` and `python tests/test_reshape_constructors.py`
cover constructors in expressions, array/section constructor items, constant
RESHAPE, and elemental MOD/MODULO. Together with DIM reduction support, these
make `xintrinsic_reductions_dim_mask_pending.f90` pass all 17 checks at O0/O2.
That checkpoint raised the sample to 6/18 passes.

`97_case_ranges.f90` and `python tests/test_case_ranges.py` cover integer CASE
lists, closed/open ranges, constant selectors, optimization and loop control.
`xselect_case_selector_list.f90` now passes at O0/O2, raising the selected sample
to 7/18 passes at that checkpoint.

`98_parameter_statements.f90` and `python tests/test_parameter_statements.py`
cover separate PARAMETER statements with preceding scalar declarations.
The external `xparam.f90` now passes at O0/O2, raising the selected sample to
8/18 passes at that checkpoint.

`99_entity_character_lengths.f90` and `python tests/test_entity_character_lengths.py`
cover per-entity CHARACTER lengths, LEN-based declaration lengths, and constant
TRIM/LEN_TRIM. The external `xcharacter_len_expr.f90` now passes at O0/O2,
raising the selected sample to 9/18 passes. Nine cases still fail compilation.

`100_cshift.f90` and `python tests/test_cshift.py` cover scalar circular shifts,
multidimensional DIM, sections, empty arrays and overlapping WHERE assignments.
`xwhere_mask_capture_pending.f90` now passes at O0/O2, raising the selected
sample to 10/18 passes (eight compile failures).

`101_nested_where.f90` and `python tests/test_nested_where.py` cover nested
multidimensional masks, ELSEWHERE precedence, captured masks, empty arrays and
uninitialized elements outside the enclosing mask. The external
`xwhere_nested_elsewhere_pending.f90` now passes at O0/O2, bringing the sample
to 11/18 passes (seven compile failures). All four selected WHERE cases pass.

`102_reshape_options.f90` and `python tests/test_reshape_options.py` cover
RESHAPE PAD repetition and constant ORDER permutations, including rank-three
results, empty arrays and overlapping inputs. These implement the first two
checks in `xintrinsic_transform_order_pending.f90`; the full probe still needs
other transformations and remains a compile failure. The sample stays at
11/18 passes.

`103_matrix_transforms.f90` and `python tests/test_matrix_transforms.py` cover
TRANSPOSE and array-valued CSHIFT, including sections, rank-three arrays,
overlapping inputs and empty arrays. The first five checks in the transformation
probe are now supported. EOSHIFT, SPREAD, PACK and UNPACK still prevent the full
probe from compiling, so the selected sample remains at 11/18 passes.

`104_eoshift.f90` and `python tests/test_eoshift.py` cover end-off shifts with
scalar/array SHIFT and BOUNDARY, default boundaries, extreme shifts, empty arrays
and overlapping inputs. The first six transformation checks now pass in isolation.
SPREAD, PACK and UNPACK still block the full probe; the selected sample remains
at 11/18 passes.

`105_spread.f90` and `python tests/test_spread.py` cover scalar/array replication,
all insertion positions for matrix sources, nested calls, constructors and empty
results. The first eight transformation checks now pass in isolation. PACK and
UNPACK still block the complete probe; the selected sample remains at 11/18 passes.

`106_pack_unpack.f90` and `python tests/test_pack_unpack.py` cover PACK with VECTOR
or literal masks, and UNPACK with scalar/array FIELD. Tests include vector tails,
aliases, inactive undefined elements and runtime errors for short vectors.
The external transformation probe now has one unsupported operation: PACK with
a runtime mask and no VECTOR. With that one assertion omitted, its other ten
assertions pass; the unmodified selected suite remains at 11/18 passes.

`107_runtime_pack.f90` and `python tests/test_runtime_pack.py` cover runtime-mask
PACK without VECTOR. Logical result lengths are distinct from storage capacity,
with runtime conformance checks for comparisons and assignments. The original
`xintrinsic_transform_order_pending.f90` now passes all eleven assertions at
O0/O2, bringing the selected sample to 12/18 passes (six compile failures).

`108_locations.f90` and `python tests/test_locations.py` cover MINLOC/MAXLOC/FINDLOC,
DIM, MASK, KIND and BACK, including empty selections and nondefault lower bounds.
`xintrinsic_locations_dim_back_pending.f90` now passes all fifteen checks at
O0/O2, raising the selected sample to 13/18 passes (five compile failures).

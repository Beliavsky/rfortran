# ofort compatibility coverage

Run the broader external inventory:

```bat
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --discover --jobs 4 --report target/ofort-expanded.json
```

Discovery considers `.f90`/`.F90` files in `tests/cases` that have a sibling
`.out` file or a name ending in `_pending`. Saved output identifies candidates;
gfortran supplies the expected output. The runner does not import or execute the
ofort Python tests. This covers standalone fixture files, not generated tests,
REPL tests, or the full pytest suite.

The report inventories every Fortran source in that directory and records a
reason for each exclusion. Discovery conservatively excludes temporary sources,
initialization/undefined-read diagnostic probes, calls involving randomness,
timing or process control, argument/environment queries, standard-input READ,
includes, missing companion modules, and the resource-dependent 40 GB allocation
probe. These categories need explicit inputs, companion sources or tailored
oracles. The scan is a heuristic rather than a Fortran parser; an explicit
`--selection` can include a reviewed case that discovery excludes.

Each discovered gfortran reference is run twice in fresh directories. Reference
build/run failures, runtime stderr and unstable output are recorded separately
and do not count as rfort failures. Stable reference output is compared with
rfort at both O0 and O2. Passing these checks does not establish that a source
is standard-conforming or free of undefined behavior.

Use `--filter` to focus on a filename glob, for example:

```bat
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --discover --filter "xcharacter*" --jobs 4 --report target/ofort-character.json
```

`--jobs` runs independent cases concurrently (default 1); each compilation and
execution retains its own directory. Reports remain ordered by source name and
include per-optimization outcomes, approximate feature groups based on filenames,
and failure groups based on diagnostics. Full stdout/stderr and source/compiler
hashes remain available for investigating individual failures.

## Quick sample

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
directories. The quick selection consists of standalone, deterministic
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
checks output values rather than precise formatting, including for discovered
I/O cases. Format-layout conformance needs separate tests in the ordinary
regression suite. Reports retain unnormalized stdout/stderr, source SHA-256 hashes, compiler
binary hash/path, and reference compiler version for investigation.

## Recorded results (Windows x64)

### Expanded inventory (2026-10-10)

[`expanded-baseline.json`](expanded-baseline.json) records the broader snapshot.
The checkout contained 768 Fortran sources: 474 candidates were attempted and
294 were excluded with recorded reasons. Of the candidates, 440 built and ran
successfully with a stable, stderr-free gfortran reference.

| Outcome | Cases |
| --- | ---: |
| rfort passes at both O0/O2 | 44 |
| rfort compilation fails at both O0/O2 | 391 |
| rfort output differs at both O0/O2 | 5 |
| Reference compilation fails | 31 |
| Reference execution fails | 2 |
| Reference execution crashes | 1 |

No rfort crashes, timeouts or optimization-dependent outcomes occurred in this
run. The 391 compile failures comprise 309 parse errors, 55 other compile errors,
18 explicitly unsupported features and 9 name-resolution errors. These coarse
groups classify diagnostics rather than count independent missing features.

Four output differences exposed vector-subscript correctness problems:
`xvec_sub.f90`, `xvec_sub_order.f90`,
`xvector_subscript_expr_pending.f90`, and
`xvector_subscript_read_pending.f90`. Reads lose elements; the overlapping swap
`x([1,2]) = x([2,1])` produces `[20,20,30]` instead of `[20,10,30]`, and a scalar
fill updates only the first selected element. These are fixed in the vector
checkpoint below. The fifth difference, `xkinds_supported.f90`, is the documented
capability difference for REAL128: rfort returns -1, while gfortran provides
kind 16. It is not evidence of an incorrect arithmetic result.

The original quick selection still passes **18/18** through the revised runner.
Run `python tests/test_ofort_compat_runner.py` for 12 local reporting/discovery
tests, which do not require the external checkout. Full compiler/reference logs
and individual source hashes are in `target/ofort-expanded.json`; the tracked
snapshot keeps outcome/exclusion groups, compiler identity, a source-manifest
hash and the five output comparisons. Discovery remains a fixture inventory,
not a complete ofort-suite or Fortran-conformance measure.

### Vector-subscript fixes

The vector-subscript checkpoint fixes all four execution bugs found in the
expanded baseline. [`vector-selection.json`](vector-selection.json) selects
eight external vector probes, adding scalar/constructor fills, section RHS
values, reductions and self-indexed assignments to the four original failures.
Run them with:

```bat
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --selection tests/ofort/vector-selection.json --strict --jobs 4 --report target/ofort-vectors.json
```

The local `117_vector_subscripts.f90` fixture and
`python tests/test_vector_subscripts.py` also check interpreter execution,
nondefault bounds, repeated read indices, empty selections, scalar operand
capture, loop-condition reevaluation, numeric kinds, duplicates, invalid indices
and selected-element initialization. The earlier expanded snapshot remains the
pre-fix baseline; the REAL128 capability difference remains unchanged.

The rebuilt release compiler passes all **8/8 vector probes at O0/O2** against
gfortran. `target/ofort-vector-regressions.json` records **52/52 passes** at both
levels: these eight probes and the 44 previously passing expanded-baseline
cases. At that checkpoint the full discovery inventory had not been rerun.

### Numeric model inquiries (2026-10-11)

[`numeric-model-checkpoint.json`](numeric-model-checkpoint.json) records a fresh
full discovery run with the debug compiler: 474 candidates, 294 exclusions, and
440 stable, stderr-free gfortran references. The refreshed compiler before these
changes passed 58 cases; after the changes, **62/440 pass at both O0 and O2**,
with no lost passes. The remaining outcomes are 372 compile failures and six
output differences. There are no rfort crashes, timeouts or differences between
optimization levels. Reference failures comprise 31 compile failures, two runtime
errors and one timeout, and remain separate from rfort results.

New inventory passes are `xdigits.f90`, `xepsilon.f90`, `xmodulo.f90`, and
`xtype_bound.f90`. Empty list-directed `PRINT *` now emits a blank record.
Numeric model inquiries support REAL kinds 4/8 and the applicable INTEGER kinds
1/2/4/8, including `X=` and PARAMETER initialization. Type inquiries do not read
unset argument values. The large-real output crash exposed by `xepsilon.f90`
is fixed, including finite DOUBLE PRECISION output at the upper range boundary.

The focused selection includes `xradix.f90`, which discovery excludes because
it has neither saved output nor a pending suffix. Run the three original inquiry
fixtures with the default release compiler:

```bat
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --selection tests/ofort/numeric-model-selection.json --strict --jobs 3 --report target/ofort-numeric-model.json
```

The release compiler also passes all **63/63** selected regressions at O0/O2:
the 62 passing inventory cases plus `xradix.f90`. Full logs are in
`target/ofort-model-release-regressions.json`. The local fixture
`124_numeric_model.f90` and `python tests/test_numeric_model.py` cover models,
result kinds, scope/import rules, invalid arguments, empty PRINT boundaries,
unset arguments and large real output through interpreter/native O0/O2 execution
with gfortran comparisons. All nine focused tests pass with debug and release
compilers; the local suite passes 82 Rust unit tests and 124 Fortran fixtures.

Fixing empty PRINT also lets five bit-intrinsic probes reach native execution:
`xbtest.f90`, `xiand.f90`, `xibclr.f90`, `xibits.f90`, and `xibset.f90` omit
their intrinsic call results from output. These are outstanding code generation
bugs that were previously hidden by parse failures, now fixed in the bit-intrinsic
checkpoint below. The sixth output difference
is the known REAL128 capability difference in `xkinds_supported.f90`.
`xprecision.f90` and `xrange.f90` still stop on their unsupported COMPLEX
examples; their INTEGER/REAL model behavior is covered by the local tests.

### Scalar integer bit intrinsics (2026-10-11)

[`bit-checkpoint.json`](bit-checkpoint.json) records **68/68 selected passes**
at O0/O2 with debug and release compilers: the previous 63 regressions and
`xbtest.f90`, `xiand.f90`, `xibclr.f90`, `xibits.f90`, and `xibset.f90`.
These five original ofort output failures now match gfortran. Run them with:

```bat
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --selection tests/ofort/bit-selection.json --strict --jobs 4 --report target/ofort-bits.json
```

Use `--selection tests/ofort/bit-regression-selection.json` to run all 68
regressions recorded at this checkpoint.

Scalar BTEST/IAND/IOR/IEOR/NOT/IBSET/IBCLR/IBITS/ISHFT now retain INTEGER
kinds 1/2/4/8 and handle sign bits, full-width shifts, and zero-length fields.
Constant and runtime control arguments receive bounds checks. Named arguments,
PARAMETER folding, PURE procedures, scalar array elements, and initialization
checks are covered. Whole-array calls, INTEGER(16), and ISHFTC remain unsupported.

`python tests/test_bit_intrinsics.py` passes eight focused tests through the
interpreter and native O0/O2 code, with gfortran comparisons, using both compiler
profiles. The local suite passes 83 Rust unit tests and 125 Fortran fixtures.
Full selected-release logs are in `target/ofort-bit-regressions-release.json`.

The full discovery inventory has not been rerun after this change. Its five
bit-output differences are resolved; the previously recorded REAL128 difference
and 372 compile failures have not been reassessed by this selected run.

### Impure elemental procedures (2026-10-11)

`ximpure_elemental.f90` now passes against gfortran at O0/O2. The release
compiler retains all 68 passes from the bit-intrinsic selection, for **69
selected passes**. `target/ofort-impure-elemental-release.json` records those
cases and `xelemental.f90`, which now compiles and runs but still fails the
output comparison because DOUBLE PRECISION arrays print only six significant
digits. Its scalar DOUBLE PRECISION output retains greater precision.
The full discovery inventory has not been rerun.

IMPURE ELEMENTAL subroutines now expand supported rank-one array calls in
element order. Constructors, constant sections, scalar input broadcasting,
writable array arguments, OPTIONAL arguments, and named arguments are covered.
Ordinary ELEMENTAL procedures retain their implicit PURE restrictions. The
new local fixture is `126_impure_elemental.f90`; `python
tests/test_impure_elemental.py` passes five focused tests with debug and release
builds through interpreter/native O0/O2 execution, including gfortran checks.
The local suite passes 84 Rust unit tests and 126 Fortran fixtures.

### Quick-sample history

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

`109_intrinsic_env.f90` and `python tests/test_intrinsic_env.py` cover intrinsic
ISO_FORTRAN_ENV numeric kind imports, ONLY, re-export, host association and
explicit intrinsic/non-intrinsic module selection. This supplies a dependency
of both pending numeric probes; integer kind suffixes and further numeric
semantics still need work. The selected sample remains at 13/18 passes.

`110_double_precision_names.f90` and `python tests/test_double_precision_names.py`
cover DOUBLE/PRECISION as identifiers while preserving the DOUBLE PRECISION type
phrase. The original mixed-kind probe now gets past its `double` declaration;
both numeric probes still require integer kind-suffixed literal support.

`111_integer_literals.f90` and `python tests/test_integer_literals.py` cover
integer kind suffixes, literal range checks, integer promotion, numeric KIND
expressions and exponentiation precedence. Both pending numeric probes now
pass at O0/O2, raising the selected sample to 15/18 passes (three compile
failures). Remaining probes cover declaration initialization, optional CHARACTER
arguments and overlapping CHARACTER array substring assignments.

`112_main_initialization.f90` and `python tests/test_main_initialization.py`
cover constant main-program declaration initializers, mutable values, array
constructors/fills, explicit bounds and diagnostics for other storage scopes.
The original `xarray_init.f90` now passes at O0/O2, bringing the selected sample
to 16/18 passes. Optional CHARACTER arguments and CHARACTER array overlap remain.

`113_optional_procedures.f90` and `python tests/test_optional_procedures.py` cover
optional scalar functions/subroutines, PRESENT, absent-argument forwarding and
scalar CHARACTER arguments with per-call lengths. The original `xoptional.f90`
now passes at O0/O2 against gfortran. The isolated release report
`target/ofort-optional-procedures.json` has 17/18 passes; overlapping CHARACTER
array/substring assignment is the remaining compile failure.

`114_character_comparisons.f90` and `python tests/test_character_comparisons.py`
cover scalar CHARACTER relations, blank padding, concatenations, TRIM, empty
strings and loop-condition reevaluation. This fixes a prerequisite for checking
overlap results; the remaining external probe still needs CHARACTER array
storage and substring assignment support.

`115_scalar_substrings.f90` and `python tests/test_scalar_substrings.py` cover
fixed-bound scalar substring reads and overlapping assignments, including both
copy directions from the original overlap probe. The complete external case
still needs CHARACTER array storage and array substring assignments.

`116_character_arrays.f90` and `python tests/test_character_arrays.py` cover
fixed rank-one CHARACTER arrays, overlapping sections and array substring
assignments, scalar fills, comparisons, empty arrays and initialization checks.
The original `xcharacter_overlap_assignment_pending.f90` now passes at O0/O2
against gfortran. The rebuilt release report `target/ofort-character-arrays.json`
records **18/18 selected cases passing**, with no remaining failures in this
sample. This is the selected compatibility sample, not the entire ofort suite.

## COMPLEX sample

`complex-selection.json` selects seven ofort programs covering literals,
constructors (including an implied-DO), component access, magnitude, roots,
component kinds, arithmetic, conjugation, dot products and complex/real matrix
multiplication. All seven passed with the debug and release compilers at O0 and O2 against
gfortran on Windows during the COMPLEX implementation. Reports are generated
locally; they are not the full inventory result.

```bat
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --compiler target/debug/rfort.exe --selection tests/ofort/complex-selection.json --report target/ofort-complex.json --strict
```

The output comparator recognizes numeric `(real,imaginary)` pairs with optional
internal spaces and compares both components using its existing real tolerance.

## DOT_PRODUCT sample

`dot-product-selection.json` selects `xfast_dot_product.f90` (which compares a
real dot product with `SUM(g*d)`) and `xnumeric_complex_semantics_pending.f90`.
The compiler uses checked reductions for numeric and logical dot products and
retains conjugation for complex inputs. Local tests additionally cover type/kind
promotion, empty vectors, sections, generic dispatch and runtime length errors.

```bat
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --compiler target/debug/rfort.exe --selection tests/ofort/dot-product-selection.json --report target/ofort-dot-product.json --strict
```

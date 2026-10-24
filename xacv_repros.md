# xacv.f90 compilation repros

Baseline repros checked on 2026-10-23 using the then-existing release rfort
executable and `gfortran -std=f2018`. The initial investigation changed neither
the compiler nor `xacv.f90`; the subsequent compiler fix is recorded below.

These five standalone programs originally isolated two bug families. All five
compile and run with gfortran.

After the intrinsic-generic fix, the four intrinsic-extension repros compile
and run with rfort as well. They are regression fixtures in
`tests/programs/`, exercised by `python tests/test_intrinsic_generics.py`.
The whole derived-type array-element assignment repro still fails and is
retained as a standalone example of the remaining limitation.

The former array-operator blocker at line 1494,
`ascending = all(vec(2:) > vec(:n-1))`, is now fixed for supported elemental
operators. The sixth repro,
[tests/programs/xacv_elemental_derived_comparison.f90](tests/programs/xacv_elemental_derived_comparison.f90),
compiles and prints `T` with rfort and gfortran. Array comparisons, scalar
broadcasting in both orders, numeric/CHARACTER results, ranks through 15,
runtime shape conformance and uninitialized-component behavior are covered by
`python tests/test_elemental_operators.py`.

The derived-type `INTENT(OUT)` dummy in `set_time_of_day`, around line 940,
is now supported. The seventh repro,
[tests/programs/xacv_elemental_derived_out.f90](tests/programs/xacv_elemental_derived_out.f90),
prints `9 30 0` with rfort and gfortran. Regression tests in
`python tests/test_elemental_derived_subroutines.py` cover scalar and array
OUT/INOUT, OPTIONAL derived dummies, keywords, nested scalar records, sections,
and writable array elements without premature reads of unset components.

The derived-type result of ELEMENTAL function `tod_int`, at line 958, is now
supported. The eighth repro,
[tests/programs/xacv_elemental_derived_result.f90](tests/programs/xacv_elemental_derived_result.f90),
prints `9 30 45` with both rfort and gfortran. Regression tests in
`python tests/test_elemental_derived_results.py` cover scalar and fixed-vector
results, nested calls, OPTIONAL/keywords, self-assignment, impure side effects,
default initialization, early returns, nested records, and scalar reduction
arguments.

The `date_time_vec_vec` blocker (around lines 538-541) is now fixed: its
allocatable `dd(:)` component has element type `date_time_vec`, which itself
owns allocatable `xx(:)` storage. The ninth repro,
[tests/programs/xacv_nested_allocatable_records.f90](tests/programs/xacv_nested_allocatable_records.f90),
prints `2` with both rfort and gfortran. Owned numeric/LOGICAL component
descriptors preserve independent allocation, bounds, initialized-element masks,
deep scalar-element copies and cleanup. Regression tests are in
`python tests/test_allocatable_record_arrays.py`, including rank-15 descriptors.

The former line-722 blocker, `num_obs(dvec(ivec))`, is now fixed. The actual
argument is a scalar array element whose record owns an allocatable component.
The tenth repro,
[tests/programs/xacv_elemental_allocatable_argument.f90](tests/programs/xacv_elemental_allocatable_argument.f90),
prints `3` with rfort and gfortran. Scalar ELEMENTAL calls now use the ordinary
array-component interface after record flattening. Regression tests also cover
whole scalar actuals, fixed array components, and impure ELEMENTAL INOUT calls.
Array-valued calls involving these record types remain explicitly unsupported.

The former line-1172 blocker in `month_year_code_vec`,
`xmy(i) = month_year_code_scalar(text(i))`, is now fixed. The eleventh repro,
[tests/programs/xacv_derived_result_array_element.f90](tests/programs/xacv_derived_result_array_element.f90),
prints `2024 2024 1 2` with rfort and gfortran. Ordinary scalar derived-valued
function calls are materialized once before splitting their components for
array assignment. `tests/test_scalar_record_results.py` also checks side
effects, scalar broadcasts, constructor items, keywords, self-referencing
arguments, multidimensional targets and nested/CHARACTER/fixed array components.

The former line-2560 blocker,
`call groups_alloc(yyyymmdd(xx%date),ipos)`, is now fixed. Selecting a scalar
derived component across a record array creates a view of its primitive
component storage. The twelfth repro,
[tests/programs/xacv_nested_component_array.f90](tests/programs/xacv_nested_component_array.f90),
prints `2024 2025` with rfort and gfortran. Tests in
`tests/test_derived_component_views.py` cover nested selections, sections,
vector subscripts, multidimensional parents, array assignments and writable
array dummy arguments. The writable-view reference was checked with gfortran
`-fcheck=all,no-array-temps` because its ordinary `-fcheck=all` warns about
array temporaries for these arguments.

The former declaration blocker at line 4154 in `describe_date_time_vec_vec`,
`type(date_time), pointer :: z(:)`, is now supported for types with scalar
numeric/LOGICAL components, including nested scalar records. The thirteenth
repro,
[tests/programs/xacv_derived_array_pointer.f90](tests/programs/xacv_derived_array_pointer.f90),
prints `2 2024 2025` followed by `2024 2030` with rfort and gfortran.
`tests/test_derived_array_pointers.py` covers ranks 2, 3 and 15, target identity,
strided aliases, declared lower bounds, mixed NULLIFY, keyword ASSOCIATED,
write-through, pointer dummy escape and preservation of uninitialized fields.
`tests/test_pointer_association.py` also checks ASSOCIATED target sections with
non-unit lower bounds: their bounds must stay in declared coordinates.

The former association blocker at line 4162, `z => xx%dd(isym)%xx`, now uses
borrowed allocation descriptors. The pointer shares the record-owned data,
initialization masks, extents and lower bounds without copying the allocation.
The fourteenth repro,
[tests/programs/xacv_owned_component_pointer.f90](tests/programs/xacv_owned_component_pointer.f90),
prints `2 2024 2025` with rfort and gfortran. Matching-shape component writes
preserve existing buffers and pointer identity. Tests in
`tests/test_owned_record_pointers.py` cover bounds, masks, identity, zero-size
and runtime-sized allocations, ranks through 15, nested date/time records,
local pointer dummy escape, TARGET requirements and local-target lifetime
checks. General standalone ALLOCATABLE pointer targets remain unsupported.

The former line-4271 blocker, `dt%date = dates`, is now supported as an
assignment to a component-array view. The fifteenth repro,
[tests/programs/xacv_component_array_assignment.f90](tests/programs/xacv_component_array_assignment.f90),
prints `2024 2025` with rfort and gfortran. Regression tests in
`tests/test_component_array_assignment.py` cover array and scalar assignment,
sections, vectors, overlap, nested paths, ranks through 15, runtime allocation,
pointer parents, function results, mixed field layouts, sibling preservation,
initialization checks and INTENT(IN) protection.

The former line-4281 blocker contains `trim(str(minval(times)))` and
`trim(str(maxval(times)))`. Nested calls now lower each actual once, keeping
component flattening separate from expression evaluation.
The sixteenth repro,
[tests/programs/xacv_nested_derived_reduction.f90](tests/programs/xacv_nested_derived_reduction.f90),
extends MINVAL for a two-component time type and nests its derived result in
an ELEMENTAL CHARACTER-valued STR call. It prints `03:30` with rfort and gfortran.
Tests in `tests/test_scalar_record_results.py` also cover nested numeric consumers,
keyword/OPTIONAL arguments, constructors, subroutine actuals and ELEMENTAL record
results, including single evaluation of impure functions.

The former line-15947 blocker is formatted output of `dates(i,:)`.
The seventeenth repro,
[tests/programs/xacv_derived_matrix_row_output.f90](tests/programs/xacv_derived_matrix_row_output.f90),
prints `01/02/2024 03/04/2025` with rfort and gfortran. Direct output loops handle
triplets and vector selections without copying whole component arrays. Tests in
`tests/test_derived_section_output.py` cover runtime bounds/strides, nonunit bounds,
record/component order, nested records, array components, ranks through 15,
initialization/bounds checks and single evaluation of subscript expressions.

The former scalar derived-type blocker at line 1046 is now supported:
`tmin = merge(t1,t2,t2>t1)`. The eighteenth repro,
[tests/programs/xacv_derived_merge.f90](tests/programs/xacv_derived_merge.f90),
preserves the ELEMENTAL function and overloaded comparison and prints `3 30 0`
with rfort and gfortran. Scalar MERGE selects the complete record after evaluating
its mask once, reusing derived assignment for supported component kinds. Tests
in `tests/test_derived_merge.py` cover aliases, nested records, managed components,
array elements, scalar broadcasts, constructors, procedures and invalid types.

The former full-source blocker at line 1854 is now supported:
`call set_alloc(pack(dates,is_weekday(dates)),week_dates)`. The nineteenth repro,
[tests/programs/xacv_pack_derived_array.f90](tests/programs/xacv_pack_derived_array.f90),
passes PACK of an allocated record array and an ELEMENTAL mask to an
allocatable-output subroutine. Both rfort and gfortran print `1 3`.
PACK retains its record-array type/rank through component lowering and gathers
selected records using one captured mask. Tests in `tests/test_derived_pack.py`
cover source ranks through 15, sections, constructors, nested records, fixed array
components, runtime VECTOR tails, empty selections, mask shape/type checks and
single evaluation. Related fixes preserve ELEMENTAL result metadata during
allocatable assignment and return native LOGICAL literal function results correctly.
Owned allocatable record components remain unsupported for PACK.

The former internal list-directed READ blocker at line 2124 is now supported:
`read (text,*,iostat=ierr_read) (dummy,i=1,nskip_col_),date`. The twentieth repro,
[tests/programs/xacv_read_derived_record.f90](tests/programs/xacv_read_derived_record.f90),
skips two fields before reading a record with three integer components. Both
rfort and gfortran print `1 2 2024`. Scalar record input now expands writable
components in declaration order without reading the old record value. Tests in
`tests/test_derived_read.py` cover nested records, fixed array components,
constant formats, shared input sessions, runtime implied-DOs and input status.
Components with ALLOCATABLE or POINTER attributes still require defined I/O.

The former full-source blocker at line 2603 is now supported:
`call unique_dates_vec([xx,yy],zz)`. The twenty-first repro,
[tests/programs/xacv_concat_derived_arrays.f90](tests/programs/xacv_concat_derived_arrays.f90),
concatenates two assumed-shape record arrays as an INTENT(IN) actual and copies
the result to an allocatable output. Both rfort and gfortran print `2 2024 2025`.
Constructors gather complete records with shared column-major indexing rather
than treating each array-valued item as one scalar. Tests in
`tests/test_derived_constructors.py` cover mixed/nested/empty items, runtime
multidimensional sources, ranks through 15, function evaluation, overlapping
assignment, component arrays, output/inquiries and PACK. Owned allocatable
components and constructor implied-DOs remain unsupported.

The later-pass blocker at source line 722 is now supported:
`num_obs(dvec(ivec))`. The twenty-second repro,
[tests/programs/xacv_elemental_allocatable_component_size.f90](tests/programs/xacv_elemental_allocatable_component_size.f90),
asks an ELEMENTAL function for the size of an allocatable derived-array component
of a scalar record selected from an array. Both rfort and gfortran print `2`.
Numeric/LOGICAL ALLOCATABLE function dummies now use descriptor association,
retaining allocation status, bounds and caller-owned storage. Early allocation
checks distinguish descriptor association from array-data reads. Tests in
`tests/test_allocatable_function_dummies.py` cover unallocated/empty actuals,
component ranks through 15, element kinds, bounds and writable function dummies.

The former full-source blocker at line 6550 is now supported:
`xdiag(j:j+n1-1) = diag(xx(:,:,i3))` reported `array xx requires 2 subscripts`.
The twenty-third small example,
[tests/programs/xacv_diag_forall.f90](tests/programs/xacv_diag_forall.f90),
retains the nested diagonal functions and FORALL used by this code. With input
`2`, gfortran prints `1 4 5 8`. The smaller example currently stops earlier with
`FORALL currently requires one array-element or array-section assignment` in
`diag`; replacing its FORALL by a DO loop lets the standalone example compile.
It exercises the related diagonal-function lowering path but does not yet
isolate the full source's exact rank diagnostic.

The twenty-fourth repro isolates that exact diagnostic and is now a regression:
[tests/programs/xacv_function_dummy_descriptor_shadow.f90](tests/programs/xacv_function_dummy_descriptor_shadow.f90).
It needs no FORALL, derived types, array-valued results or runtime input. A main
program declares an unrelated rank-two ALLOCATABLE array `xx`; a module function
has its own rank-three dummy also named `xx`, and passes `xx(:,:,1)` to a function
with a rank-two dummy. Both rfort and gfortran now print `7`. Previously, renaming
only the main-program `xx` to `unrelated` avoided the incorrect rank diagnostic.
Function-argument preparation inherited the main-program descriptor metadata
even after replacing the binding with the function's local declaration.

`character_elementals::Lower::body` now clears inherited metadata for locally
declared names before loading their own declarations and attributes. Bindings
actually inherited from a host retain their descriptors. The regression suite
`tests/test_function_scope_metadata.py` checks the exact repro, scalar shadowing,
dummy ranks through 15, local allocation bounds, CHARACTER dummy lengths and
inherited arrays in BLOCKs,
using the interpreter, native code at -O0/-O2 and gfortran as a reference.
The next former blocker was `ALLOCATABLE output specialization requires a
bounded rank-one array input or fixed local array`, now supported below.

The twenty-fifth repro isolates this former blocker and is now a regression:
[tests/programs/xacv_read_words_allocatable.f90](tests/programs/xacv_read_words_allocatable.f90).
It reduces `read_words_line`, defined at line 4935 and called at line 25771 as
`read_words_line(iu,cstats_levels)`. The procedure reads a count from a line,
allocates an assumed-length CHARACTER output array, and reads the words into it.
The main program uses a scratch file, so no input or external data is needed.

```cmd
rf.bat tests/programs/xacv_read_words_allocatable.f90 --run
```

Native rfort, its interpreter and gfortran with `-std=f2018 -fcheck=all` now
print four lines: `3`, `alpha`, `beta`, `gamma`. CHARACTER allocatable dummies
without an input array or fixed local supplying a storage bound now borrow the
caller's heap descriptor. Caller arrays passed to these dummies also receive
heap storage, including ones initially allocated to a small constant extent.
The existing bounded CHARACTER result/copy paths remain available.

`tests/test_character_allocatable_dummies.py` checks the exact repro, optional
counts, empty results, forwarding, repeated allocation, nonunit bounds,
INTENT(INOUT), INTENT(OUT) deallocation on entry, zero-length elements and a
runtime extent of 4097. It compares interpreter and -O0/-O2 native output with
gfortran. After this fix, the full source reached the former diagnostic
`READ into vector-subscript selections is not yet supported`, reported at line
16948 on `call set_alloc((dates(:nread)),dates)`, fixed below.

The twenty-sixth repro isolates this former misleading READ diagnostic:
[tests/programs/xacv_read_named_module_section.f90](tests/programs/xacv_read_named_module_section.f90).
It has no READ statements, derived types, OPTIONAL arguments or vector
subscripts. It passes a rank-one triplet section to an ordinary copying
subroutine in a module named `read_copy_mod`. Gfortran with
`-std=f2018 -Wall -fcheck=all` prints `1 2 3`.

```cmd
rf.bat tests/programs/xacv_read_named_module_section.f90 --run
```

Native rfort and its interpreter now print `1 2 3` too. Previously, renaming only
`read_copy_mod` to `copy_mod` avoided the error. The vector passes classified any
generated call whose name started with `@` and contained `read` as input.
Module-qualified user procedure names could meet that test too; the rejected
argument was an ordinary triplet section. The full-source diagnostic reached
this guard with sections of the flattened month/day/year component arrays
passed to `set_alloc`, rather than READ items.

Both vector passes now use `input::transfer_start` to identify canonical READ
operations and distinguish their transfer items from control arguments. User
procedure names no longer affect READ classification. The regression suite
`tests/test_read_call_classification.py` covers module/procedure names containing
`read`, fixed and runtime vector actuals, writable triplet sections and retained
diagnostics for genuine READs with unsupported vector destinations, using the
interpreter, native -O0/-O2 and gfortran for valid source.
The full source now reaches line 25805, calling `print_stats` with
`good=abs(xx-bad_value) > 1.0d-10`, and reports `type or rank mismatch for good`.

Renaming the module, putting `read` in the procedure name instead, and passing
the whole input array provided controls. Initial failing results and controls
are saved in the ignored build directory at
`target/acv-repros/read-classification/report.json`.

The twenty-seventh repro isolates the former `good` argument mismatch at line 25805:
[tests/programs/xacv_runtime_logical_mask_actual.f90](tests/programs/xacv_runtime_logical_mask_actual.f90).
It passes `abs(xx-bad_value)>1.0e-10_dp` as an optional rank-two LOGICAL
INTENT(IN) argument, with `xx` an allocated DOUBLE PRECISION matrix. No generic
interface, derived type, CHARACTER argument or file input is needed.

```cmd
rf.bat tests/programs/xacv_runtime_logical_mask_actual.f90 --run
```

Gfortran with `-std=f2018 -Wall -fcheck=all` prints `2 2 3`: the matrix extents
and the number of true mask elements. Native rfort and its interpreter now print
the same result. Previously they reported `type or rank mismatch for good` at
the call, while passing the same expression via an allocated LOGICAL matrix
worked. Both actual arguments have the same Fortran type, kind and rank.

Positional calls, required dummies, direct comparisons without ABS and fixed
matrix inputs now work too. Multidimensional expression actuals are evaluated
into owned arrays of the correct type, kind and rank using ordinary array
assignment, before argument association. Fixed extents use fixed storage;
runtime extents use heap descriptors. Shape inference follows elemental calls
and scalar broadcasting, including scalar-first MIN/MAX and kind conversions.
The shared runtime assignment path also recognizes elemental MIN/MAX.

`tests/test_multidimensional_expression_arguments.py` checks the repro and its
controls, integer kinds 1/2/4/8, REAL/DOUBLE PRECISION, LOGICAL expressions,
user-defined elemental results, ranks through 15, empty arrays, nonunit source
bounds, forwarding, repeated calls and runtime shape mismatch checks. Valid
cases run in the interpreter, at -O0/-O2 and with gfortran as reference.
Initial failing results are recorded in the ignored build directory at
`target/acv-repros/good-mask/report.json`.
The next former blocker was `runtime PACK results are not yet supported as
arguments to @optional_65`, reported at line 25198 on the `print_square_matrix`
call involving `corr_mat(xx)` and `labels(:size(xx,2))`, now fixed below.

The twenty-eighth repro isolates that nested function call and is now a regression:
[tests/programs/xacv_nested_runtime_array_function.f90](tests/programs/xacv_nested_runtime_array_function.f90).
It contains no PACK, labels, formatted output or correlation calculation. The
three procedures retain the relevant structure: `outer` takes an assumed-shape
matrix, `make_matrix` returns an automatic matrix with shape derived from that
input, and `consume` accepts the result as an assumed-shape argument.

```cmd
rf.bat tests/programs/xacv_nested_runtime_array_function.f90 --run
```

Gfortran with `-std=f2018 -Wall -fcheck=all` prints `2 2 4`, the result shape
and sum. Rfort's native and interpreter modes now print the same result.
Previously, the call inside `outer` failed with the runtime-PACK diagnostic;
calling directly from main or assigning the result to a local array first worked.
Removing OPTIONAL did not avoid the original error.

An isolated diagnostic compiler confirmed that the full-source `@optional_65`
is `statistics_mod::corr_mat` with its optional `good` omitted. The rejected
argument is a runtime `@packed_view` of `xx`, not an explicit PACK result.
The original error occurred while passing `xx` into `corr_mat`, before the outer
`print_square_matrix` call could consume its result. Initial tracing lives under
`target/acv-repros/diagnostic-compiler`. Reference runs and controls are recorded in
`target/acv-repros/runtime-pack-call/report.json` (ignored build directory).

The subroutine expansion pass now defers argument association when a known
function call remains in the actuals. The existing alternating function pass
first evaluates that call with the wrapper's bound descriptors, then outer
association uses the resulting array. The runtime-PACK validation itself is
unchanged. `tests/test_nested_array_function_arguments.py` checks the repro,
fixed/empty inputs, required and OPTIONAL interfaces, two nested function levels,
array expressions containing results, genuine PACK inputs, single evaluation,
changing result shapes, and INTEGER,
REAL, DOUBLE PRECISION and LOGICAL results through rank 15 against gfortran.
The full source then reached line 25856, `mean(ret_use**2)`, reporting that
reduction extents and section bounds must be constant integers because
`@deferred_extent` is not a previously defined integer PARAMETER.

The twenty-ninth repro isolates that pointer-expression argument and is now a regression:
[tests/programs/xacv_pointer_expression_actual.f90](tests/programs/xacv_pointer_expression_actual.f90).
It retains a pure scalar `mean` function with an ordinary assumed-shape
INTENT(IN) dummy and the actual `ret_use**2`. A pointer selects elements 2 and 3
of a fixed TARGET array containing `[1,2,3]`. The target deliberately needs no
allocation, avoiding the separate unsupported ALLOCATABLE-target association.

```cmd
rf.bat tests/programs/xacv_pointer_expression_actual.f90 --run
```

Both rfort modes now print `6.500`, agreeing with gfortran. Pointer expressions
use owned array temporaries with runtime extents before ordinary read-only
argument association. Bare pointer arrays can also supply ordinary INTENT(IN)
array dummies. Pointer assignment views retain heap temporaries' logical shape
instead of treating their flattened backing declaration as the actual shape.
`tests/test_pointer_expression_arguments.py` checks pure and ordinary functions,
bare pointers, strides, reassociation, empty sections, numeric kinds, LOGICAL
comparisons, ranks 2/3/15, explicitly allocated destinations and runtime errors
against gfortran in interpreter and native O0/O2 modes.

General ALLOCATABLE pointer targets, writable ordinary pointer actuals and
automatic reallocation of an unallocated destination from a pointer expression
remain separate limitations. Initial reference runs and controls are recorded
in `target/acv-repros/pointer-expression/report.json` (ignored).
The full source now reports `array cstats_indep_ is used before ALLOCATE` at
line 21084, `call set_alloc(["min","max"],cstats_indep_)`.

## Intrinsic calls after extending an intrinsic generic

`date_mdy_mod` in `xacv.f90` extends MAX, MINVAL, MAXVAL and INT for
derived types. The intrinsic operations on built-in types must remain available.
The integer MAX call at original line 835 was the initial full-source blocker.

| Repro | Originally rejected built-in call | Expected output |
| --- | --- | --- |
| [xacv_max_intrinsic.f90](tests/programs/xacv_max_intrinsic.f90) | `max(1,mod_year)` inside a PRESENT guard | `1 5 2024` |
| [xacv_minval_intrinsic.f90](tests/programs/xacv_minval_intrinsic.f90) | `minval(years)` for an integer array | `1 2020` |
| [xacv_maxval_intrinsic.f90](tests/programs/xacv_maxval_intrinsic.f90) | `maxval(years)` for an integer array | `5 2024` |
| [xacv_int_intrinsic.f90](tests/programs/xacv_int_intrinsic.f90) | `int(x)` for a real scalar | `3 2024` |

Each program exercises both a built-in call and the derived-type overload.
The reduction-specific procedures use component assignments to avoid mixing in
the separate whole-element assignment failure described below.
As controls, removing the generic interface and calling its specific procedure
directly for the derived-type arguments makes all four programs compile and
produce the expected output with rfort. The built-in calls are left unchanged.

The original errors were `no matching specific procedure for generic ...`.
The former REPEAT-only fallback has been replaced by the shared fallback in
`src/intrinsic_generics.rs`, called by `src/operators.rs`. User-specific
selection and ambiguity checks precede intrinsic fallback. Constant processing
now defers user-procedure calls until selection, preserving custom keywords.

## Whole derived-type array-element assignment

[xacv_derived_array_element.f90](tests/programs/xacv_derived_array_element.f90) reduces
`yy = xx(1)` in `minval_date_mdy`/`maxval_date_mdy`. It needs neither a
generic interface nor a function: a fixed-size local array is sufficient.

```fortran
date = dates(1)
```

Expected output: `2020`. Current rfort error:

```text
derived assignment to `date` requires a value of type `date_type`
```

A version using an assumed-shape dummy array and derived-type function result
also fails. For this one-component type, replacing the whole assignment with
`date%year = dates(1)%year` compiles and prints `2020` with rfort.
This identifies an independent lowering/type-recognition problem for the
whole array element; it is not restricted to assumed-shape arguments.

## Commands (Windows CMD)

Run each repro with the existing compiler build:

```cmd
rf.bat tests/programs/xacv_max_intrinsic.f90 --run
rf.bat tests/programs/xacv_minval_intrinsic.f90 --run
rf.bat tests/programs/xacv_maxval_intrinsic.f90 --run
rf.bat tests/programs/xacv_int_intrinsic.f90 --run
rf.bat tests/programs/xacv_derived_array_element.f90 --run
```

For an independent reference check, for example:

```cmd
gfortran -std=f2018 tests/programs/xacv_max_intrinsic.f90 -J target -o target/xacv_max_gfortran.exe
target/xacv_max_gfortran.exe
```

All repros are self-contained and need no external data. Expected output is
also recorded in each source file. Initial failing diagnostic and reference-run results are saved in the ignored build directory at
`target/acv-repros/report.json`.

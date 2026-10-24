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
checks. Standalone ALLOCATABLE TARGET arrays are covered by the thirty-third
regression below.

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

Writable ordinary pointer actuals and
automatic reallocation of an unallocated destination from a pointer expression
remain separate limitations. Initial reference runs and controls are recorded
in `target/acv-repros/pointer-expression/report.json` (ignored).
The full source then reported `array cstats_indep_ is used before ALLOCATE` at
line 21084, `call set_alloc(["min","max"],cstats_indep_)`.

The thirtieth repro isolates that local CHARACTER allocatable output and is now a regression:
[tests/programs/xacv_character_allocatable_output.f90](tests/programs/xacv_character_allocatable_output.f90).
A module procedure declares an unallocated local `character(len=20)` array and
passes it to an ALLOCATABLE INTENT(OUT) dummy. The callee explicitly allocates
and fills the output; the caller is not required to allocate it beforehand.

```cmd
rf.bat tests/programs/xacv_character_allocatable_output.f90 --run
```

Native and interpreter modes now agree with gfortran (`-std=f2018 -Wall -fcheck=all`)
and print:

```text
2 20
min|max
```

Storage selection now records all supported ALLOCATABLE dummy positions when
selecting descriptors for ordinary caller locals, including bounded CHARACTER
copy routines. Function results retain their separate bounded/heap storage
policy. This fixes both the unallocated-local diagnostic and the related
preallocated-local `ALLOCATABLE dummy requires an ALLOCATABLE actual` error.
`tests/test_character_allocatable_dummies.py` checks pure and ordinary copying,
generic/optional interfaces, reallocation, empty inputs, element lengths 0/1/20
and repeated local calls in interpreter and native O0/O2 modes against gfortran.
Existing CHARACTER function-result tests verify the bounded result path.
Initial controls and reference runs are saved in
`target/acv-repros/character-allocatable-output/report.json` (ignored).
The full source then reached line 24134,
`forall (icol=1:size(xx,2)) xacf(:,icol) = acf_vec(xx(:,icol),nlags)`, reporting
`FORALL bounds and stride must be scalar INTEGER expressions`.

The thirty-first repro isolates that FORALL bound and is now a regression:
[tests/programs/xacv_forall_size_bound.f90](tests/programs/xacv_forall_size_bound.f90).
A pure matrix-valued function uses `size(xx,2)` as the column-loop bound and
calls a pure vector-valued function on each column section. A runtime-sized
allocated matrix supplies the actual input.

```cmd
rf.bat tests/programs/xacv_forall_size_bound.f90 --run
```

Interpreter and native O0/O2 output agrees with gfortran: `1 2 3 4 5 6`.
FORALL's scalar-integer classifier now recognizes the integer value inside
an allocation-checked inquiry, and the bound helper used for LBOUND/UBOUND is
accepted by its purity check. The runtime allocation guards are preserved.
`tests/test_runtime_forall.py` covers pure and ordinary array functions, fixed
and allocated inputs, SIZE/LBOUND/UBOUND controls, negative strides, keyword
arguments with default integer kind, empty rows/columns, and unallocated
inquiries, alongside the existing snapshot and invalid-control tests.
The full source then passed this FORALL and reported
`output implied-DO bounds and step require supported scalar INTEGER expressions`.
That diagnostic originally had no source location.

The thirty-second repro isolates that output implied-DO diagnostic and is now a regression:
[tests/programs/xacv_output_implied_do_bound.f90](tests/programs/xacv_output_implied_do_bound.f90).
A procedure copies an assumed-shape CHARACTER argument into a runtime-allocated
local array, then combines implied-DO output from both arrays in one WRITE.
This mixed local/dummy pattern also appears in `print_many_stats_vec_str`,
which prints `cstats_indep_` and `cstats` together.

```cmd
rf.bat tests/programs/xacv_output_implied_do_bound.f90 --run
```

Both native and interpreter modes now agree with gfortran
(`-std=f2018 -Wall -fcheck=all`) and print `min max min max`.
Output's scalar-integer classifier recognizes allocation-checked extents and
lower/upper bound helpers without discarding their runtime guards. SIZE,
LBOUND and UBOUND controls can therefore survive delayed argument association.
Formatted implied-DO validation errors now retain the enclosing statement's
source location; generated array-view implied-DO errors do likewise.

Original controls, which still work in both rfort modes and gfortran:

- Replace `allocate(local(n))` with the fixed `allocate(local(2))`.
- Store `size(local)` in a scalar INTEGER and use that variable as the bound.
- Split the two implied-DOs into separate WRITE statements (two output lines).

A simple numeric SIZE bound, a single CHARACTER implied-DO, and output of a whole
allocated CHARACTER array also work in isolation. The mixed array combination
remains in the regression. `tests/test_formatted_implied_do.py` checks the repro,
runtime formats, PRINT and WRITE, numeric/LOGICAL element types and kinds,
LBOUND/UBOUND, negative strides, empty inputs, unallocated-array errors and the
line number of invalid controls at native O0/O2 and in the interpreter against
gfortran. Initial controls and reference runs are saved in
`target/acv-repros/output-implied-do-bound/report.json` (ignored).
The full source then passed this output and reported
`pointer association to ALLOCATABLE storage is not yet supported`.

The thirty-third repro isolates that pointer target limitation and is now a regression:
[tests/programs/xacv_allocatable_pointer_target.f90](tests/programs/xacv_allocatable_pointer_target.f90).
It retains the `ret_use => zret(i1:)` association from original line 25825.
The main program declares `zret` with both ALLOCATABLE and TARGET, allocates
three elements, associates a pointer with the last two, and updates the first
selected element through the pointer. It nullifies the pointer before freeing
the target, avoiding any use of a dangling association.

```cmd
rf.bat tests/programs/xacv_allocatable_pointer_target.f90 --run
```

Both rfort modes now agree with gfortran (`-std=f2018 -Wall -fcheck=all`) and
print, ignoring real formatting:

```text
2
20 3
1 20 3
```

Both whole allocations and sections now borrow the target's data and
initialization mask. TARGET arrays retain runtime extents and lower bounds;
unallocated association is guarded at runtime. Heap-backed pointer expressions
use runtime snapshot storage for overlap safety. ASSOCIATED keeps its original
target section instead of comparing against a vector-selection copy.
`tests/test_allocatable_pointer_targets.py` checks numeric/LOGICAL kinds,
derived arrays with scalar numeric fields, ranks through 15, non-default bounds,
strides, zero extents, matching-shape target updates, returned associations,
initialization sharing, readonly/lifetime errors and unallocated targets against
gfortran in interpreter and native O0/O2 modes.
The original fixed TARGET control still works. Initial controls and reference runs are saved in
`target/acv-repros/allocatable-pointer-target/report.json` (ignored).
The next diagnostic was at line 16994,
`write (*,fmt_cr) "min",minval(xx,dim=1)`. This DIM-reduction limitation is now
covered by the regression below.

## Runtime-sized DIM reductions

[tests/programs/xacv_runtime_dim_reduction.f90](tests/programs/xacv_runtime_dim_reduction.f90)
is a self-contained regression for `MINVAL(xx,DIM=1)` at line 16994. It allocates
a matrix using runtime extents and prints column minima, row maxima and column
sums:

```text
11 21 31
31 32
23 43 63
```

```cmd
rf.bat tests/programs/xacv_runtime_dim_reduction.f90 --run
```

The fix preserves logical axes before flat storage lowering and reuses the
shared reduction implementation. Numeric, logical and integer-bit reductions,
NORM2 and MINLOC/MAXLOC/FINDLOC now accept supported runtime-sized arrays through
rank 15 with a constant DIM. Tests cover all axes, empty slices, masks and
uninitialized masked-out elements, kinds, location ties/BACK, non-default bounds,
sections, assumed-shape dummies and result assignment in interpreter and native
O0/O2 modes, with gfortran comparisons. Run
`python tests/test_runtime_dim_reductions.py`. Runtime-valued DIM remains
unsupported; CSHIFT/EOSHIFT/SPREAD retain their separate fixed-shape restrictions.
Earlier call specialization can still reject a DIM result from a runtime-sized array passed directly
to a procedure; assigning it to an ALLOCATABLE temporary first avoids that path.

After the DIM fix, the full source reported this diagnostic at line 16965:

```fortran
if (all(dates(2:) < dates(:nread-1))) then
```

```text
runtime PACK results are not yet supported as arguments to `date_mdy_lt`
```

The overloaded comparison passes runtime array views to an elemental procedure.
The early reduction pass was dropping the procedure's ELEMENTAL result-type
metadata, so it treated the call as an unsupported ordinary array call. That
metadata is now preserved and the repro below is a regression test.

[tests/programs/xacv_runtime_derived_comparison.f90](tests/programs/xacv_runtime_derived_comparison.f90)
reproduces the error with a one-field derived type, an elemental overloaded `<`,
and an allocated array of three descending values. Expected output is `T`;
gfortran `-std=f2018 -Wall -fcheck=all` compiles it without warnings and produces
that output. Both rfort native O0/O2 builds and interpreter mode now compile it
and produce the same output.

```cmd
rf.bat tests/programs/xacv_runtime_derived_comparison.f90 --run
```

The scalar comparison `dates(2) < dates(1)` succeeds. Printing the elemental
array comparison or the direct `date_mdy_lt(dates(2:),dates(:nread-1))` call also
succeeds and produces `T T`. ALL around either form now succeeds too.
`python tests/test_runtime_elemental_reductions.py` also checks ANY/COUNT/PARITY,
empty sections, numeric result kinds, nested calls, DIM reductions, masks,
locations and IMPURE ELEMENTAL functions against gfortran in both rfort modes.
Initial controls and reference results are saved in
`target/acv-repros/runtime-derived-comparison/report.json` (ignored).

After the date-comparison fix, the full source reported
`unsupported expression in numeric array reduction` at line 21187. This is now
covered by the regression below:

```fortran
xmask = xindep <= thresh(1)
```

[tests/programs/xacv_runtime_array_scalar_comparison.f90](tests/programs/xacv_runtime_array_scalar_comparison.f90)
is a self-contained repro using runtime-allocated actual arguments, assumed-shape
REAL dummy arrays and an automatic LOGICAL result array. Both rfort modes now
compile it and print `T T F`, matching gfortran `-std=f2018 -Wall -fcheck=all`.
The generated checked-subscript function for `thresh(1)` now retains its integer
result type during scalarization. The same type metadata is retained by output
lowering and the other public array-expression helpers.

```cmd
rf.bat tests/programs/xacv_runtime_array_scalar_comparison.f90 --run
```

Replacing `thresh(1)` with the literal `2d0` works. Reading `thresh(1)` into a
scalar local first and then comparing also works:

```fortran
bound = thresh(1)
xmask = xindep <= bound
```

A control doing the original comparison directly in the main program, with a
fixed-size LOGICAL destination, also compiles. These controls narrowed the
failure to the scalar array-element operand within the specialized
array-expression path, rather than to the comparison operator itself.
`python tests/test_array_comparisons.py` now checks the supported intrinsic
comparison operators across built-in types and all six overloaded derived-type
relations, with scalar/array operands, sections, runtime dummies, blank padding,
empty arrays, and ranks through 15. Related fixes preserve packed CHARACTER copy
coordinates and query COMPLEX section extents without reading array values.
Initial controls and
reference results are saved in
`target/acv-repros/runtime-array-scalar-comparison/report.json` (ignored).

The rebuilt release passed these comparison failures in the full source. Before
the inquiry changes below, its next diagnostic was at line 15482:

```fortran
inquire (file=xfile,exist=tf)
```

```text
inquire: unsupported I/O control `exist`
```

This is reported by the later I/O-control lowering pass. The release diagnostic
is saved in `target/acv-repros/xacv-comparisons-release.txt` (ignored).

### INQUIRE file existence (now a regression)

Standalone repro: [xacv_inquire_exist.f90](tests/programs/xacv_inquire_exist.f90).
It retains the contained `file_exists` function from the original code, creates
a scratch file, checks its existence, deletes it, then checks again.

```cmd
rf.bat tests\programs\xacv_inquire_exist.f90 --run
```

Expected output (verified with gfortran using `-std=f2018 -Wall -fcheck=all`):

```text
 T
 F
```

The original release rejected line 19, `inquire(file=xfile, exist=tf)`, with
`inquire: unsupported I/O control ` followed by the control name `exist`.
The shared lowering and runtimes now support EXIST, ACTION, NAMED, IOMSG and
ERR alongside the existing inquiry controls. `python tests/test_inquire.py`
checks the repro and inquiry states in the interpreter, native O0/O2 and gfortran.
The original failure was verified in an isolated temporary directory;
results are saved in `target/acv-repros/inquire-exist/report.json` (ignored).
When running the reference program manually, use a scratch directory: it creates
and deletes `rfort_inquire_exist_test.tmp` there.

The rebuilt release passes the inquiry lowering in `rfort check xacv.f90`.
The next diagnostic is at line 15203:

```fortran
open (unit=iunit,file=xfile,action="write",position="append",iostat=ierr)
```

```text
OPEN currently supports position='rewind'
```

This is a later lowering pass despite the earlier source line. The check log
is saved in `target/acv-repros/xacv-inquire-release.txt` (ignored). The release
inquiry suite passes in the interpreter, native O0/O2 and gfortran; its log is
`target/inquire-release-tests.txt` (ignored).

### OPEN append positioning (now a regression)

Standalone repro: [xacv_open_append.f90](tests/programs/xacv_open_append.f90).
It creates a file containing `first`, reopens it with the same OPEN controls as
`xacv.f90:15203`, appends `second`, then reads both records and deletes the file.

```cmd
rf.bat tests\programs\xacv_open_append.f90 --run
```

Expected output, also verified with gfortran:

```text
first
second
```

The shared lowering and both runtimes now support initial append positioning.
The native runtime seeks after opening, rather than using C append mode, so a
subsequent REWIND permits ordinary reads and overwriting. Failure to seek is an
OPEN error and does not publish a connection or assign NEWUNIT.
`python tests/test_open_position.py` covers fixed units and NEWUNIT, existing
and missing files, replace semantics, read-only opens at EOF, unformatted stream
files, scratch files, ASIS/REWIND, subsequent positioning and IOSTAT/IOMSG.
Run the standalone program in a scratch directory: it creates and deletes
`rfort_open_append_test.tmp` in its working directory.

The rebuilt release passes this OPEN in `rfort check xacv.f90`. The next
diagnostic is at line 14210, on the continued `sump = (((( &` assignment:

```text
read of uninitialized variable `@matrix_alloc_xx_0_length` before assignment
```

This names compiler-generated allocation-length storage. The full-source check
log is `target/acv-repros/xacv-open-position-release.txt` (ignored).
All nine positioning tests pass with the rebuilt release in the interpreter,
native O0/O2 and gfortran; results are saved in
`target/open-position-release-tests.txt` (ignored).

### Scalar/matrix name collision through an elemental call (now a regression)

Standalone repro:
[xacv_scalar_matrix_name_collision.f90](tests/programs/xacv_scalar_matrix_name_collision.f90).
It reduces the `calck1` failure at original line 14210 to an elemental function
calling an elemental subroutine. The subroutine assigns a local scalar `xx`
before using it; the main program independently declares an allocatable
rank-two matrix also called `xx`.

```cmd
rf.bat tests\programs\xacv_scalar_matrix_name_collision.f90 --run
```

Expected output, verified with gfortran `-std=f2018 -Wall -fcheck=all` without
warnings:

```text
  1.2500
```

Before the fix, both native compilation and interpreter execution rejected line 18:

```fortran
answer = xx + 1d0
```

```text
read of uninitialized variable `@matrix_alloc_xx_0_length` before assignment
```

Two controls compile and run with rfort, each printing the expected result:
renaming only the main program's matrix to `matrix`, or removing ELEMENTAL from
both procedures. These controls implicate array metadata leaking across scopes
during elemental-call lowering. The two source entities named `xx` are separate
and both are initialized in the repro.
The call-expansion pass now removes an inherited binding and all owner-tagged
metadata when a local declaration shadows it, then reloads that scope's own
metadata. This happens both when entering a scope and before associating a
specialized callee's arguments, so stale caller descriptors cannot be used
while specializing the callee body. The repro now prints the expected result.
`python tests/test_array_scope_metadata.py` checks caller matrices of different
intrinsic types/ranks, local fixed arrays, scalar dummies, pointer metadata,
assumed-shape argument extents and genuine uninitialized scalar reads.
An extracted copy of the original Bessel module also passes the source check.
The rebuilt release passes the original scope collision in full object
compilation of `xacv.f90`. The next diagnostic is at line 4965:

```fortran
write (*,*) (trim(words(i)) // " ",i=1,nread_)
```

```text
read of uninitialized variable `@alloc_block_215` before assignment
```

This is a separate generated-storage diagnostic on CHARACTER implied-DO output.
The full-source log is `target/acv-repros/xacv-scope-release.txt` (ignored).
All 151 scope, elemental, module, pointer and allocation tests pass against the
rebuilt release, including interpreter and native O0/O2 checks with gfortran
references. The nine OPEN-position tests also pass. Logs are
`target/scope-metadata-release-tests.txt` and
`target/scope-metadata-open-tests.txt` (ignored); all 132 Rust unit tests pass.
Validation and control sources are saved under
`target/acv-repros/matrix-scalar-shadow/` (ignored), including `report.json`.

### Allocatable CHARACTER implied-DO output storage (now a regression)

Standalone repro:
[xacv_allocatable_character_implied_do.f90](tests/programs/xacv_allocatable_character_implied_do.f90).
It reduces `read_words_line` at original line 4965 to a subroutine with an
assumed-length allocatable CHARACTER output. Both branches allocate `words`;
the positive branch fills every element, then makes the second word distinct.
The implied-DO prints the populated array after the branches.

```cmd
rf.bat tests\programs\xacv_allocatable_character_implied_do.f90 --run
```

Expected output, verified with gfortran `-std=f2018 -Wall -fcheck=all` without
warnings:

```text
 alpha beta
```

Before the fix, both native compilation and interpreter execution rejected line 21:

```fortran
write(*,*) (trim(words(i)) // ' ', i=1,nread)
```

```text
read of uninitialized variable `@alloc_block_5` before assignment
```

The temporary's serial number differs from `@alloc_block_215` in the full
source. Initializing `i=1` immediately before this WRITE is a useful control:
it made both rfort modes run, but they incorrectly printed `alpha alpha`.
Initializing the implied-DO index beforehand should not be necessary and does
not change the expected output. This points to materialization of `words(i)`
before loop initialization, followed by reuse of the first item across iterations.
The fix expands output implied-DOs before CHARACTER temporary materialization
and function expansion, so each indexed item is evaluated after the loop index
is assigned. Both modes now print `alpha beta`, including with `i=1` or `i=9`
set before the WRITE. Uninitialized-element checks remain active.

`tests/test_character_output_implied_do.py` also covers PRINT, formatted/internal
WRITE, constant and variable file units, nested and reverse loops, empty loops,
zero steps, per-iteration function calls, once-only function-valued bounds and
unit capture before output-item evaluation, and output inside SELECT CASE in
array-valued functions. Procedure-local validation now discovers declarations
generated inside case branches. The suite compares interpreter and native O0/O2
results with gfortran where applicable.

Validation ran in an isolated temporary directory. Results and the initialized
index control are saved under `target/acv-repros/character-implied-do-storage/`
(ignored), including `report.json`.

All 11 focused tests pass against the rebuilt release, including interpreter
and native O0/O2 runs with gfortran references. The 60 adjacent output and scope
tests also pass, as do all 132 Rust unit tests. Logs are
`target/character-implied-do-release-final.txt`,
`target/character-output-release-tests.txt` and
`target/character-output-rust-tests.txt` (ignored).

The rebuilt release passes the original output blocker and stops later at
`xacv.f90:20064:37`:

```fortran
case (stat_labels(iimedian))   ; xstat = median(xx)
```

```text
read of uninitialized variable `@alloc_block_4156` before assignment
```

The full-source log is
`target/acv-repros/xacv-character-output-release.txt` (ignored). This is the next
blocker; full-source compilation has not yet succeeded.

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

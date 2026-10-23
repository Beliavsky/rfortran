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

The next full-source blocker is line 4154 in `describe_date_time_vec_vec`:
`type(date_time), pointer :: z(:)`, followed by `z => xx%dd(isym)%xx`.
Derived-type pointer arrays remain unsupported. The thirteenth repro,
[tests/programs/xacv_derived_array_pointer.f90](tests/programs/xacv_derived_array_pointer.f90),
isolates the declaration, association and write-through behavior. It prints
`2 2024 2025` followed by `2024 2030` with gfortran; rfort rejects the pointer
array declaration.

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

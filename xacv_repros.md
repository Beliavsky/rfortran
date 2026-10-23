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

The next full-source blocker is the derived-type `INTENT(OUT)` dummy in
`set_time_of_day`, around line 940. A seventh repro is
[tests/programs/xacv_elemental_derived_out.f90](tests/programs/xacv_elemental_derived_out.f90).
It fails with rfort and prints `9 30 0` with gfortran. A scalar call suffices;
ELEMENTAL subroutine dummy validation is the immediate limitation.

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

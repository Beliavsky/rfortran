# Issues in the external utility source

These observations apply to the source hash recorded in `baseline.json`. The
coverage work does not change the external module.

## Recursive combination cache

`choose_func` and `choose_func_long` read `mm(n,k)` to decide whether a cached
answer exists. The module declares `integer(kind=long_int) :: mm(100,100)` without
an initializer, and these paths do not initialize it before reading it. Static
storage being zero in a particular executable is not a Fortran initialization.

Calling `choose_func(5,2)` produces rfort's uninitialized-element runtime error;
gfortran happened to return 10 in the initial probe. That difference is not used
as evidence of an rfort arithmetic bug. The `combinatorics` driver checks defined
boundary paths of these two functions, plus general cases of `choose`, `factorial`
and `factorial_long`. General recursive paths need an initialized cache in the
original module before they can have a defined-output test.

## write_trim_vec suffix

The implementation assigns `suffix_` using `opt=prefix`, rather than `opt=suffix`.
The baseline exercises its current behavior with matching delimiters. Agreement
with gfortran tests compiler consistency, not whether this library implements its
intended API correctly. Distinct prefix and suffix behavior needs a library fix
and an independent expected-value assertion.

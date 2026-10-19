# Fortran 2023 additions

The compiler currently implements the following additions. Standard selection is
still a partial compatibility check; see [usage](usage.md#standard-selection).

## Trimmed CHARACTER output

The `AT` edit descriptor outputs a CHARACTER item after removing its trailing
blanks. Leading blanks and nonblank characters are preserved. An all-blank or
zero-length value contributes no characters. Unlike `A`, `AT` cannot have a field
width or precision. It can have a repeat count and occur in nested or unlimited
format groups. It is only valid for output.

```fortran
character(20) :: name = "Alice"
print "('[',at,']')", name       ! [Alice]
```

This works in PRINT, formatted WRITE, internal WRITE, runtime format strings,
array output and nonadvancing output through the shared formatter. Literal
formats and FORMAT statements containing AT are diagnosed in older `-std` modes.
The revision checker does not inspect the values of runtime format variables.

## Logical kinds

`SELECTED_LOGICAL_KIND(BITS)` accepts a scalar INTEGER expression, including a
runtime expression or a keyword argument. It returns a default INTEGER. The
current implementation supports one LOGICAL kind, kind 4 with a 32-bit language
storage model: requests of at most 32 bits return 4, and larger requests return -1.

The result can be used in `LOGICAL(kind=...)` or `LOGICAL(...)` declarations when
it is constant. Scalar and array declarations, function results, and derived-type
components accept the selector. Other LOGICAL kind values are rejected explicitly.

The following new ISO_FORTRAN_ENV constants report current capabilities:

| Constant | Value | Meaning |
|---|---:|---|
| LOGICAL8 | -2 | This size is unsupported; a larger LOGICAL size exists. |
| LOGICAL16 | -2 | This size is unsupported; a larger LOGICAL size exists. |
| LOGICAL32 | 4 | Supported LOGICAL kind. |
| LOGICAL64 | -1 | This size and larger LOGICAL sizes are unsupported. |
| REAL16 | -2 | 16-bit REAL is unsupported; larger REAL sizes exist. |

These additions do not introduce additional storage kinds. `LOGICAL_KINDS`
continues to contain just kind 4. Imports, renaming and re-export through modules
are supported. The new inquiry and constants require Fortran 2023 in revision
checking modes; an ordinary explicit `LOGICAL(4)` declaration remains valid in
earlier modes.

## Delimiter search

`CALL SPLIT(STRING, SET, POS [, BACK])` finds the next delimiter from SET
and updates the scalar INTEGER variable POS. STRING and SET accept scalar
CHARACTER expressions. Keyword arguments and calls from PURE procedures are
supported. POS may also be an integer array element.

For forward searches, start with POS = 0. A match returns its one-based position;
no match returns LEN(STRING) + 1. Subsequent calls start after the preceding
position. With BACK = .TRUE., start with POS = LEN(STRING) + 1; searches start
before POS and return 0 when no delimiter remains. Adjacent, leading, and trailing
delimiters can therefore delimit empty tokens. Empty strings and empty delimiter
sets are accepted. Invalid starting positions produce a runtime error.

```fortran
integer :: pos = 0
call split("one,two", ",", pos)
print *, pos                       ! 4
```

The intrinsic is available in the interpreter and native compiler and requires
`-std=f2023` when a standard is selected. Currently only the default CHARACTER
kind is supported, as elsewhere in the compiler.

## Conditional expressions and arguments

Fortran 2023 parenthesized conditional expressions use this syntax:

```fortran
value = (x > 0 ? x : y > 0 ? y : z)
```

Conditions must be scalar LOGICAL expressions. They are evaluated in order until
one is true; its following expression supplies the result. If none is true, the
final expression supplies the result. No later condition or unselected expression
is evaluated. All alternatives must have the same declared type, kind and rank.
CHARACTER lengths may differ. Nested conditional expressions are supported.

Conditional actual arguments also preserve the identity of a selected variable,
so they can be passed to an INTENT(OUT) or INTENT(INOUT) dummy. Every non-absent
consequent must satisfy the dummy's requirements, even if its condition is a
constant false. Consequents must agree in POINTER and ALLOCATABLE attributes.

```fortran
call update((x > 0 ? x : y), amount=(enabled ? increment : .nil.))
```

Here `.NIL.` means that the corresponding OPTIONAL dummy is absent. It is only
permitted as a consequent of a conditional argument in a procedure reference.
A consequent may contain a conditional expression, but cannot be another
conditional argument containing `.NIL.`.

Choices work in the interpreter and native compiler, including array and derived
values, named and generic calls, constant initializers, automatic array bounds,
loop conditions, and output/constructor implied-DO loops. These features use the
compiler's existing type, array, and procedure support; they do not add general
polymorphism or assumed-rank execution. Selecting a standard requires
`-std=f2023` for either conditional form.

## Try it

```cmd
rf.bat -std=f2023 tests\programs\xfortran2023_basics.f90 --run
rf.bat -std=f2023 tests\programs\xconditional.f90 --run
python tests/test_fortran2023.py
python tests/test_split.py
python tests/test_conditionals.py
python tests/test_standards.py
```

The feature tests use the interpreter and native code at O0/O2. Expected logical
kind values describe rfort's capabilities and should not be compared directly
with another compiler that supports a different set of LOGICAL kinds.

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

## Try it

```cmd
rf.bat -std=f2023 tests\programs\xfortran2023_basics.f90 --run
python tests/test_fortran2023.py
python tests/test_standards.py
```

The feature tests use the interpreter and native code at O0/O2. Expected logical
kind values describe rfort's capabilities and should not be compared directly
with another compiler that supports a different set of LOGICAL kinds.

# rfort on the Fortitude test fixtures

Run: 2026-10-17, with a debug build of rfort at commit `7abb07f`.

Fortitude checkout: `C:\rust\public_domain\github\fortitude`, commit
`9592c3e992ce8a0e53ee97bbd1598d8a67cdad92` (2026-10-13).

Nothing from Fortitude was copied into this repository. The fixtures were read
in place, and only file names, line numbers and short quoted source lines appear
below. [FORTITUDE_REUSE.md](../FORTITUDE_REUSE.md) covers licensing if fixtures
are later copied.

## Method

[fortitude_fixtures.py](fortitude_fixtures.py) runs three commands on each of
the 125 Fortran files in the checkout:

- `gfortran -fsyntax-only`, to tell valid Fortran from the fixtures that are
  deliberately invalid or need modules from other files;
- `rfort parse`, which runs the lexer and parser;
- `rfort check`, which also runs lowering and semantic checks.

The raw results are in [fortitude_fixtures.json](fortitude_fixtures.json). To
rerun:

```sh
python evaluation/fortitude_fixtures.py C:/rust/public_domain/github/fortitude target/debug/rfort.exe target/fortitude.json
```

These are lint fixtures. They show which programs rfort accepts, not whether it
compiles them correctly, and many deliberately use old or nonstandard forms.

## Summary

| | Files | rfort parses | rfort checks |
|---|---|---|---|
| gfortran accepts | 86 | 36 (42%) | 24 (28%) |
| gfortran rejects | 39 | 7 | 1 |

The rest of this report covers the 86 files gfortran accepts. All 50 parse
failures and 12 check failures have a cause in the tables below.

## Parse failures

| Cause | Files | Example | Kind |
|---|---|---|---|
| `implicit none (type, external)` (Fortran 2018) | 19 | `correctness/C003.f90:6` `implicit none (type)` | Missing feature |
| Obsolescent or deleted features: labelled DO, COMMON, EQUIVALENCE, BLOCK DATA, ENTRY, PAUSE, arithmetic IF | 7 | `obsolescent/OB011.f90:2` `common /mydata/ i, j` | Missing feature |
| Empty source file | 5 | `include-test/a.f90` (0 bytes) | Bug |
| A module or external procedure after the main program in the same file | 3 | `correctness/C001.f90:27` `module my_module_external` | Bug |
| CHARACTER literal continued onto the next line with `&` | 3 | `correctness/C171.f90:15` `print*, "this is a normal &` | Missing feature |
| Statements: BACKSPACE with IOSTAT=, FLUSH, the DIMENSION statement, single-line IF with ERROR STOP | 4 | `modernisation/MOD041.f90:5` `dimension y(2)` | Missing feature |
| Extensions: DOUBLE COMPLEX, `integer*8`, C preprocessor in `.F90` | 3 | `portability/PORT021.f90:1` `integer*8 function add_if(x, y, z)` | Extension |
| `type(real(kind=...))` declarations (Fortran 2008) | 2 | `correctness/C022.f90:7` `type(real(kind=kind(1.0d0))) :: bar` | Missing feature |
| DEALLOCATE with two or more objects | 1 | `correctness/C182.f90:8` `deallocate (x, y)` | Bug |
| SUBROUTINE without an argument list | 1 | `portability/PORT062.f90:23` `subroutine use_user_defined` | Missing feature |
| Bare `END` closing a module | 1 | `style/S061.f90:17` `end` | Missing feature |
| A keyword as a name (`module module`) | 1 | `style/S311_keyword_reuse.f90:1` | Bug |

The DEALLOCATE failure was a surprise, since
[language-support.md](../docs/language-support.md#deallocate) says DEALLOCATE
accepts lists. The lexer reads `(x, y)` after `deallocate` as the start of a
COMPLEX literal (`ComplexOpen`), so `deallocate(a, b)` fails to parse for any
two variable names. A single object, `deallocate(a)`, works.

## Check failures

These files parse but are rejected by lowering or semantic checks.

| Cause | Files | Example |
|---|---|---|
| OPEN with ACTION= but no STATUS= (the default is `'unknown'`) | 3 | `correctness/C032.f90:4` `open(10, file="test.txt", action="read")` |
| Assumed-size dummy `x(*)` | 1 | `correctness/C071.f90` |
| POINTER with SAVE and an initializer | 1 | `correctness/C082.f90:16` `integer, pointer, save :: bar => null()` |
| ALLOCATABLE or POINTER component of derived type | 1 | `correctness/C101.f90` |
| `kind` of a COMPLEX constant in a PARAMETER | 1 | `modernisation/MOD002.f90:4` `kind((0.0d0, 0.0d0))` |
| Statement function | 1 | `obsolescent/OB001.f90:8` `CABS1( ZDUM ) = ...` |
| Specific intrinsic name `DSIN` | 1 | `obsolescent/OB031.f90:6` `y = DSIN(x)` |
| REAL kind 16 | 1 | `portability/PORT012.f90:18` `real(qp), parameter :: x6 = 9.876_16` |
| A module subroutine named `system`, like a GNU intrinsic | 1 | `portability/PORT061.f90:6` `subroutine system(x)` |
| Initialized local array (implicit SAVE) | 1 | `style/S104.f90:11` `integer, dimension(2) :: c = [6, 7]` |

The `system` case looks like a bug: a user procedure should take precedence
over an intrinsic of the same name.

## Suggested fixes, in order

1. **DEALLOCATE with a list** (bug). Fix the COMPLEX-literal check in the lexer
   so it does not apply after `deallocate`.
2. **`implicit none (type, external)`**. A grammar addition; it would fix 19
   of the 50 parse failures by itself.
3. **Program units after the main program, and empty files** (bugs). Both are
   accepted by every Fortran compiler.
4. **SUBROUTINE and CALL without parentheses, and bare `END` for a module.**
   These are common in real code and also appear in
   [feature-status.md](../docs/feature-status.md).
5. **OPEN without STATUS=**, the most common check failure.
6. **CHARACTER literals continued across lines, the DIMENSION statement,
   FLUSH, BACKSPACE with IOSTAT=**.
7. Fortran 2008 `type(real(...))`, statement functions, specific intrinsic
   names, and user procedures that share a name with an intrinsic.

The obsolescent features (COMMON, EQUIVALENCE, labelled DO and so on) and the
extensions are a separate decision about how much legacy code rfort should
accept.

Items 1–4 cover the first parse error in 30 more files, so they would let rfort
parse up to 66 of the 86 valid fixtures, up from 36. Some of those files may
then stop at a later error.

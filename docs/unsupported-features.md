# Unsupported features

> Generated file—edit [feature-status.md](feature-status.md), not this page.

Only features marked ❌ (not supported) are shown. Partially supported
features and known wrong results remain in the [full status page](feature-status.md).
Standard versions, notes, and test references are copied from that page.

Regenerate with `python tools/generate_unsupported_features.py`.
Check freshness with `python tools/generate_unsupported_features.py --check`.

## Types and kinds

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| REAL kind 16 (quad precision) | F90 | ❌ | F90 kind syntax; quad precision is optional. The REAL128 named constant is F2008. `REAL128` is -1 |  |

## Control flow

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| Assigned GOTO, arithmetic IF | F77 or earlier | ❌ |  |  |

## Input and output

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| DC decimal comma; RU, RD, RZ, RN, RC, RP rounding modes | F2003 | ❌ |  |  |
| [Defined derived-type I/O](language-support.md#file-and-standard-input) | F2003 | ❌ | Defined derived-type I/O: F2003; default component-wise derived-type I/O: F90 (covered above). |  |

## Parallel features

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| Coarrays | F2008 | ❌ |  |  |
| OpenMP and OpenACC directives | N/A | ❌ | Separate OpenMP/OpenACC specifications, not Fortran language revisions. Treated as comments |  |

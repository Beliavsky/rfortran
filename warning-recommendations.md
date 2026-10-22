# Warning recommendations for rfortran

Reviewed on 2026-10-21 against the local rfortran and Fortitude sources. This
document includes comparisons with Fortitude and the local GCC warning catalogue,
plus additional diagnostic ideas. Most entries remain proposals; implemented
checks are identified in the status section.
The initial review made no compiler changes. Implementation progress is recorded
below; unmarked recommendations remain proposals.

## Implementation status (2026-10-22)

- **Implemented: `-Wimplicit-save`.** Enabled by `-Wall`; checks procedure-local
  declaration initialization, including pointer initializers. Explicit SAVE,
  named constants, module/main-program variables, and component defaults are
  exempt. Existing saved-pointer backend limitations remain unchanged.
- **Implemented: `-Wmissing-intent`.** Opt-in, outside `-Wall`; checks data dummy
  arguments without explicit INTENT. Procedure, POINTER, and VALUE dummies are
  exempt in the initial implementation. It does not infer or insert attributes.

Both support category suppression, `-Werror`, source directives, and original
multi-file locations. See [usage](docs/usage.md#warnings), the
[example](tests/programs/xdeclaration_warnings.f90), and
[regression tests](tests/test_declaration_warnings.py).

## Existing rfortran coverage

rfortran already checks unused declarations and private module entities,
implicit typing, unreachable code, format mismatches, and optional arguments
used without proven presence. It also has some checks for character truncation,
integer literal overflow, and `INTENT(OUT)` misuse.

Relevant sources:

- [rfortran warning documentation](docs/usage.md#warnings)
- [rfortran warning categories and controls](src/unused.rs)
- [rfortran semantic checks](src/sema.rs)
- [rfortran optional-argument analysis](src/optional_warnings.rs)
- [Fortitude rule catalogue](../fortitude/docs/rules.md)

## Recommended additions

Prioritize likely mistakes over formatting preferences.

| Priority | Warning | Fortitude counterpart | Recommendation for rfortran |
|---|---|---|---|
| High | Implicit `SAVE` | C081, C082 | Warn when declaration initialization gives a procedure-local variable or pointer persistent state without explicit `SAVE`. Exempt named constants and derived-type component defaults. |
| High | Unsafe reliance on short-circuit evaluation | C161 | Extend the existing optional-argument analysis to allocation and pointer association. Warn only when an expression contains a potentially invalid access. |
| High | Ignored status results | C181 | Warn when `IOSTAT`, `STAT`, or command status is supplied but its result is discarded or overwritten without inspection or forwarding. |
| High | Accidental shadowing | C201, currently preview | Warn when a local declaration hides a host/module variable or named constant. Point to both declarations. Make this separately selectable because some shadowing is intentional. |
| Medium | Missing `INTENT` | C061 | Offer an optional warning for data dummy arguments without `INTENT`, with appropriate exceptions such as `VALUE`. Do not automatically infer and insert `INTENT(OUT)`. |
| Medium | Pointer components without default initialization | C101 | Warn about derived-type pointer components lacking `=> null()` or another valid initializer. Undefined association status makes even an `ASSOCIATED` inquiry unsafe. |
| Medium | Misleading single-line `IF` | C151, C152 | Warn about semicolons or continuations that make an unconditional statement appear conditional. This needs original source tokens, before normalization. |
| Medium | Potential precision loss | C021 provides related coverage | Improve the existing limited precision warning using actual expression types and kinds, rather than requiring a kind suffix on every real literal. |

The precision-loss recommendation also includes implicit conversions likely to
lose information, as discussed under GCC-inspired recommendations below.

### Implicit SAVE

```fortran
subroutine count_calls()
integer :: count = 0
count = count + 1
print *, count
end subroutine
```

`count` retains its value between calls. A useful warning would explain that
explicitly and suggest either explicit `SAVE` or a separate executable
initialization, depending on the intended behavior.

### Unsafe logical guards

```fortran
if (allocated(x) .and. size(x) > 0) then
```

The `.and.` does not guarantee that `SIZE(x)` is skipped when `x` is unallocated.
rfortran already recognizes the corresponding `PRESENT` problem; this would
extend that protection. Recommend nested conditionals where appropriate.

### Precision loss before conversion

```fortran
use iso_fortran_env, only: real64
real(real64) :: x
x = sqrt(2.0)
```

The calculation uses default-real precision before conversion to `real64`.
The warning should explain that, while avoiding noise for exactly representable
assignments such as `x = 1.0`. For this example, use `sqrt(2.0_real64)` when the
higher-precision calculation is intended.

## Additional compiler-oriented checks

These recommendations go beyond the Fortitude rules compared above:

- **Integer division in a real context:** explain why `x = 1/2` assigns zero to
  a real variable.
- **Nonexhaustive enumeration selection:** identify missing enumerators when
  there is no default branch. This fits rfortran's recent enumeration work
  particularly well.
- **Implicit procedure interfaces:** warn where an unavailable explicit
  interface prevents meaningful argument checking, rather than warning about
  every external procedure merely because it is external.
- **Dead stores:** warn when an assigned value is overwritten before being read,
  even if the variable's later values are used. See the proposed scope below.

### Dead stores: overwritten assignments

```fortran
x = 1.1
y = 1.2
x = 1.3
```

For ordinary local variables, warn at `x = 1.1` and point to the overwriting
assignment `x = 1.3`:

```text
warning: value assigned to `x` is overwritten before being read
```

This complements the existing assigned-but-never-read warning, which concerns
the variable as a whole. A dead-store warning concerns an individual assigned
value.

Start with conservative analysis of straight-line code:

- Warn for `x = 1.1; x = 1.3` when nothing can observe the first value.
- Do not warn for `x = 1.1; x = x + 0.2`: the second assignment reads the first
  value.
- Treat calls that might access `x` through arguments, host/module association,
  or aliases as possible uses.
- Exclude volatile variables and other externally observable storage.
- Preserve any required evaluation of the first assignment's right-hand side.
  A warning does not imply that deleting the whole statement is safe.

Later, control-flow analysis could detect values overwritten on every subsequent
path before use. Array elements and sections require checking which elements
overlap; partial overwrites must not be treated as overwriting the whole array.

Use a separate category such as `-Wdead-store`, initially opt-in. Once its
false-positive rate is low, consider adding it to `-Wall`.

## Optional policy checks and lower priorities

Keep hard-coded kind numbers, missing `USE, ONLY`, default-public modules, and
missing default branches for unrestricted integer selectors as optional
portability or coding-policy checks.

Keyword capitalization, quote style, indentation, and file organization can
remain Fortitude's responsibility.

### Inconsistent identifier capitalization

Offer an optional style warning when references use different capitalization
from the entity's declaration. Fortran treats `x` and `X` as the same
identifier, but mixed spelling can make them look like different variables:

```fortran
real :: x
x = 1.1
X = 1.2  ! warning: `X` refers to variable declared as `x`
```

A proposed category, `-Widentifier-case`, should:

- Compare references against the declared spelling of the resolved entity,
  respecting scope and local names introduced by `USE` renaming.
- Use the spelling at first occurrence for implicitly declared variables.
- Exclude keywords, comments, and string contents.
- Report once per entity and scope to avoid excessive messages.

Keep this warning outside `-Wall`, since capitalization is a style preference
and existing code may mix cases deliberately. It could belong in a separate
style-warning group. It does not prescribe uppercase or lowercase identifiers;
it checks consistency with the chosen declaration spelling.

For successive assignments such as `x = 1.1; X = 1.2`, the dead-store warning
is more important: the first value is overwritten. The capitalization warning
would explain that both assignments target the same variable.

## Optional procedure-attribute suggestions

Offer an optional analysis report identifying procedures that could be declared
`PURE`, `SIMPLE`, or `ELEMENTAL`. This would help users document intent, expose
unintended dependencies, and find scalar procedures that could operate on
arrays. Omitting these attributes is not necessarily a mistake, so keep the
suggestions outside `-Wall`.

Proposed new options:

```text
-Wsuggest-pure
-Wsuggest-simple
-Wsuggest-elemental
```

| Suggestion | Required analysis |
|---|---|
| `PURE` | Establish that the procedure satisfies purity restrictions, including restrictions on state changes, I/O, dummy arguments, and called procedures. |
| `SIMPLE` | Establish purity plus the additional restrictions on external dependencies and calls. |
| `ELEMENTAL` | Establish that the arguments, result, and body satisfy elemental restrictions. Scalar arguments alone are insufficient. |

These are overlapping properties rather than three successive levels. `SIMPLE`
implies `PURE` and can be combined with `ELEMENTAL`. Elemental procedures are
normally pure, but `IMPURE ELEMENTAL` is also allowed. `SIMPLE` should not be
combined with an explicit `PURE` prefix. See sections 15.7–15.9 of the
[Fortran 2023 interpretation document](https://j3-fortran.org/doc/year/24/24-007.pdf).

### Actionable reports and dependencies

Distinguish directly applicable suggestions from suggestions requiring changes:

```text
function `square` could be declared SIMPLE ELEMENTAL
subroutine `normalize` could be declared PURE
function `distance` could be PURE after adding INTENT(IN) to `x`
function `outer` could be PURE if `helper` is also declared PURE
```

- Analyze the call graph and handle recursive groups. Identify procedures that
  need to be annotated together: a caller cannot simply become pure while its
  called procedures remain declared impure.
- For unknown external calls or unresolved procedure pointers, report that
  eligibility cannot be established rather than making an optimistic
  suggestion. Absence of a suggestion does not prove ineligibility.
- Normally exclude explicitly `IMPURE` procedures unless the user requests
  analysis of them.
- Respect the selected language standard. `SIMPLE` is a Fortran 2023 feature;
  do not recommend it as an immediately applicable change under an older
  selected standard.
- Account for generic interfaces and uses as procedure arguments before
  recommending `ELEMENTAL`, since adding that attribute changes how a procedure
  can be called and used.

### Implementation approach

Reuse restrictions from the existing [purity](src/purity.rs),
[simple-procedure](src/simple.rs), and [elemental](src/elemental.rs) validators
where possible. Eligibility checks should operate on resolved source-level
entities before lowering erases relevant attributes or dependencies. The report
must not silently add attributes or change program semantics.

Implement `PURE` suggestions first, then `SIMPLE`, then `ELEMENTAL`. Dependency
analysis and explanations are additional work beyond reusing the validators.
Start conservatively with constructs and interfaces that rfortran can analyze
reliably, and expand coverage as its validation improves.

## GCC-inspired recommendations

The local [gfortran_warnings.txt](gfortran_warnings.txt) contains GCC's combined
warning catalogue, including options for languages other than Fortran. Select
the Fortran-relevant checks rather than treating every listed option as a
gfortran feature. The names below identify existing GCC/gfortran categories and
possible names for rfortran options; they are not implemented rfortran options.

### Correctness additions and extensions

| Priority | Category | Recommended scope for rfortran |
|---|---|---|
| High | Pointer-target lifetime (`-Wtarget-lifetime`) | Detect pointers that may outlive their targets, such as a returned pointer associated with an unsaved local array. |
| High | Conflicting argument aliasing (`-Waliasing`) | Detect conflicting associations, such as the same actual variable passed to input and output dummies. Repeated arguments alone must not trigger a warning; account for intent, accesses, overlapping sections, and applicable aliasing rules. |
| High | Information-losing implicit conversions (`-Wconversion`) | Extend the existing precision-loss recommendation to narrowing numeric kinds, real-to-integer conversion, and discarding a complex imaginary component. Avoid warning on every harmless widening conversion. |
| High | Loop-subscript analysis (`-Wdo-subscript`) | Use loop ranges to identify possible out-of-bounds accesses, including accesses inside conditional branches. Distinguish a possible invalid access from one proven to execute. |
| Medium | Intrinsic shadowing (`-Wintrinsic-shadow`) | Identify user procedures whose names hide intrinsics, such as `sum` or `size`. Make this independently selectable because the naming may be intentional. |
| Optional | Exact floating-point comparisons (`-Wcompare-reals`) | Flag equality/inequality comparisons of real or complex expressions. Keep outside `-Wall`, since exact comparisons can be intentional. |

The first four are the strongest correctness candidates. Pointer lifetime and
argument aliasing catch mistakes that ordinary type checking misses. Conversion
checking extends the existing precision-loss item rather than creating a
duplicate recommendation. Loop-range analysis complements existing bounds
checks; retain errors for accesses already proven invalid.

These categories correspond to the
[GNU Fortran warning documentation](https://gcc.gnu.org/onlinedocs/gfortran/Error-and-Warning-Options.html).

### Optional performance reports

- **Array temporaries (`-Warray-temporaries`):** identify array copies generated
  by rfortran, with original source locations and an explanation of why each
  temporary is required. Keep this report opt-in.
- **Allocatable assignment (`-Wrealloc-lhs`, `-Wrealloc-lhs-all`):** identify
  assignments that may allocate or reallocate storage. Distinguish a possible
  allocation check from an allocation known to occur. Do not imply that every
  reported assignment reallocates on every execution.
- **Function elimination (`-Wfunction-elimination`):** defer until rfortran
  performs the relevant optimizations. Report actual elimination of impure
  function calls, rather than hypothetical behavior of another compiler.

Temporary and reallocation reports should describe what rfortran's own backend
generates, not merely source constructs that could require copies in some
implementation.

### Further candidates and existing overlap

- **C interoperability (`-Wc-binding-type`):** useful as interoperability
  develops. Distinguish actual violations from portability concerns about
  default kinds.
- **Unused statement labels (`-Wunused-label`):** useful for cleaning up older
  code and likely inexpensive to implement.
- **Constant underflow (`-Wunderflow`):** flag constant calculations that lose
  magnitude, especially when a nonzero expression becomes zero.
- **Suspicious constructs (`-Wsurprising`):** provide specific diagnostics for
  cases such as an impossible `CASE (10:1)` range. Prefer individually
  controllable categories over a broad miscellaneous warning bucket.

Integer division, implicit interfaces, character truncation, unused entities,
unreachable code, and uninitialized reads are already implemented in some form
or proposed elsewhere in this document. Extend their coverage instead of
creating duplicate diagnostics. Existing definite-error checks should remain
errors.

## Suggested first implementation batch

1. Implicit `SAVE` (implemented; see status above).
2. Allocation/association guards that rely on short-circuit evaluation.
3. Accidental shadowing.

These checks catch consequential mistakes, have clear explanations, and
complement rfortran's existing warnings. New warnings should use the existing
category controls, source locations, suppression directives, and `-Werror`
support. Definite language errors and existing uninitialized-read errors should
not be downgraded to warnings.

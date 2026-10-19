# Feature status

A checklist of Fortran language features and whether rfort supports them.
[language-support.md](language-support.md) has the details, restrictions and
tests for each supported feature; the links in the **Feature** column go there.

| Mark | Meaning |
|---|---|
| ✅ | Supported in the usual forms. Restrictions may still apply; see the linked section. |
| 🟡 | Partly supported: common forms work, but notable forms are rejected. |
| ❌ | Not supported. rfort reports an error at compile time. |
| ⚠️ | Compiles, but gives wrong results in some cases. See [Known wrong results](#known-wrong-results). |

The ❌ and 🟡 marks were checked on 2026-10-17 by compiling small test programs
with rfort and comparing their output with gfortran, and by running rfort on
Fortitude's test fixtures ([report](../evaluation/FORTITUDE_FIXTURES.md)). Rows marked ✅ or 🟡
name the tests that cover them. Update this page when a feature changes.

The **Introduced** column identifies the first Fortran standard for the language
feature: F90 means Fortran 90, F95 means Fortran 95, and F2003/F2008/F2018/F2023
use the full year. **F77 or earlier** groups older features; **Mixed** means the
row combines revisions, identified in its notes; **N/A** denotes compiler behavior
or a separate specification. Labels describe the language, not the release in
which rfort implemented it, and do not claim complete `-std` enforcement.
Later extensions to a feature do not change its original introduction date.
File extensions, module-file formats, numeric kind values and the availability
of particular precisions are processor conventions rather than standard dates.

The revision labels use the [NAG standard-revision summaries](https://support.nag.com/nagware/np/r72_doc/nag_f2003.html)
([F2008](https://support.nag.com/nagware/np/r72_doc/nag_f2008.html),
[F2018](https://support.nag.com/nagware/np/r72_doc/nag_f2018.html)),
[WG5's Fortran 2023 feature summary](https://wg5-fortran.org/N2201-N2250/N2212.pdf),
and the per-procedure standard entries in the
[GNU Fortran intrinsic reference](https://gcc.gnu.org/onlinedocs/gfortran/Intrinsic-Procedures.html).

## Known wrong results

None are known. The automatic-array, `LGE`/`LGT`/`LLE`/`LLT` and `ISHFTC` bugs
found on 2026-10-17 are fixed. The compiler now reports an error for a call it
cannot generate code for, instead of using the value 0.

Three more bugs found on 2026-10-17 are also fixed:

- `print *, x*2` and other elemental expressions of a fixed-size array printed
  only their first element.
- Names were case-sensitive, so `integer :: I` and a later `i` were different
  variables, even under `IMPLICIT NONE`.
- A scalar, fixed-size array or CHARACTER variable declared in a BLOCK shared
  storage with an outer variable of the same name.
- Compiled list-directed output of a DOUBLE PRECISION array element printed
  only 6 significant digits. The interpreter still does.
- Formatted output of a constant array constructor (`write(*,'(i3)') [1,2,3]`)
  or a MATMUL result wrote only the first element.
- Assigning a value to a variable of a narrower INTEGER kind kept the whole
  value, so an INTEGER(1) variable could hold 130 and a default INTEGER could
  hold 5000000000. The value now wraps to the variable's kind, as in gfortran.
- G0 output of `FRACTION`, `SPACING`, `RRSPACING` and the other floating-point
  model functions of a REAL(4) variable printed DOUBLE PRECISION digits.
- An internal procedure of the main program that assigned a main-program
  variable changed only a local copy, so the main program kept the old value.
  Main-program variables are now host-associated.
- Assigning an expression such as `c + 1` to an unallocated multidimensional
  ALLOCATABLE array stopped with "runtime-sized array shapes do not conform"
  instead of allocating it.

Bugs found on 2026-10-18, all fixed:

- On Windows, compiled list-directed REAL output came from the C runtime's
  `_gcvt`, giving `1.e-006` and `60.`; an initialized REAL variable printed
  with 15 digits; `-0.0` printed as `0.0`; and the interpreter gave 0.0 for a
  REAL division by zero instead of an infinity or NaN.

- In compiled code, a REAL(4) variable kept double precision: a value stored
  in it was not rounded to single precision, so a sum of `0.1` ten times was
  exactly 1.0, `x = 16777217.0` kept its value, and `a == 1.0d0/3.0d0` was true
  for `a = 1.0/3.0`. Stores to REAL(4) variables, elements and dummies now
  round, and the interpreter rounds stores to REAL(4) array elements.
- Formatted output of an expression of an array constructor, as in
  `print '(*(g0,1x))', 2*[1.0, 2.0]`, wrote only the first element, and an
  elemental intrinsic of a constructor there did not compile.
- A bare `error stop` at the end of a line took the next statement, such as a
  following `case (...)`, as its stop code.

- In the interpreter (`rfort run`), storing an INTEGER value in an element of a
  REAL array, as in `x(3) = -1`, turned the whole array INTEGER, so later REAL
  values were truncated. Filling a REAL ALLOCATABLE array with an INTEGER
  (`a = 1`) did the same, and filling an INTEGER ALLOCATABLE array with a REAL
  value (`a = 2.7`) stored 0 instead of 2.
- Default component values were not applied to a fixed-size array of a derived
  type, so reading one reported an uninitialized variable.
- Assigning a function result to one of its own arguments, as in `r = r * m`
  with a defined operator or `r = f(r)`, gave wrong values when the type had an
  array component: the result was written into `r` while `r` was still being read.
- Growing an array with MOVE_ALLOC (`allocate(tmp(2*n)); tmp(1:n) = a;
  call move_alloc(tmp, a)`) stopped with "read of uninitialized variable",
  because moving the array copied its unset elements.

## Source form and declarations

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| Free source form (`.f90`, `.f95`, `.f03`, `.f08`) | F90 | ✅ |  | testsuite |
| Fixed source form (`.f`, `.for`) | F77 or earlier | ❌ |  |  |
| `INCLUDE` lines | F90 | ❌ |  |  |
| Implicit typing | F77 or earlier | ✅ | IMPLICIT COMPLEX is not supported |  |
| `IMPLICIT NONE` | F90 | ✅ |  |  |
| [`IMPLICIT NONE (TYPE, EXTERNAL)`](language-support.md#other-statement-forms) | F2018 | ✅ | EXTERNAL is accepted but not checked | test_syntax_forms.py |
| [DIMENSION statement](language-support.md#other-statement-forms) | F77 or earlier | ✅ |  | test_syntax_forms.py |
| [CHARACTER literals continued across lines](language-support.md#other-statement-forms) | Mixed | ✅ | Continued literals in fixed form: F77 or earlier; free form: F90. | test_syntax_forms.py |
| [Empty or comment-only source files](language-support.md#other-statement-forms) | N/A | ✅ |  | test_syntax_forms.py |
| [One-word keywords (`ENDIF`, `ELSEIF`, `SELECTCASE`, `DOUBLEPRECISION`, ...)](language-support.md#other-statement-forms) | Mixed | ✅ | Includes F77 keywords and F90 constructs such as SELECT CASE; spaces in keywords do not determine a separate feature. | test_syntax_forms.py |
| [BOZ literal constants](language-support.md#other-statement-forms) | F90 | 🟡 | First standardized in F90 DATA statements; additional permitted contexts were added later. A pattern over 32 bits must be converted with `INT(..., 8)` | test_syntax_forms.py |
| [`ENDFILE` and `FLUSH` statements](language-support.md#file-and-standard-input) | Mixed | ✅ | ENDFILE: F77 or earlier; FLUSH: F2003. | test_statement_forms_io.py |
| [Named IF, SELECT CASE, BLOCK and ASSOCIATE constructs](language-support.md#other-statement-forms) | Mixed | ✅ | Named IF/SELECT CASE: F90; ASSOCIATE: F2003; BLOCK: F2008. Names after END, ELSE and CASE are not checked against the construct name | test_named_constructs.py |
| [Case-insensitive names](language-support.md#other-statement-forms) | F90 | ✅ | Upper/lowercase equivalence is standardized in F90. | test_syntax_forms.py |
| [Declarations without `::`](language-support.md#declarations-without-) | F77 or earlier | ✅ | Attributes and initializers still need `::` | test_declarations_without_colons.py |
| [DIMENSION attribute](language-support.md#dimension-declarations-and-shape) | F90 | ✅ |  | test_dimension_attribute.py |
| [INTENT](language-support.md#intent-spellings) | F90 | ✅ |  |  |
| [PARAMETER attribute and statement](language-support.md#parameter-statements) | Mixed | ✅ | PARAMETER statement: F77; attribute: F90. Derived-type named constants need scalar components | test_parameter_statements.py, test_named_constants.py, test_derived_type_features.py |
| [Declaration initialization](language-support.md#declaration-initialization) | F90 | 🟡 | Saved arrays are not supported | test_main_initialization.py, test_saved_locals.py, test_initializers_and_kinds.py |
| SAVE | F77 or earlier | 🟡 | Scalars only | test_saved_locals.py |
| VALUE attribute | F2003 | ✅ |  |  |
| PROTECTED attribute | F2003 | ❌ |  |  |
| CONTIGUOUS attribute | F2008 | ✅ | Accepted |  |
| VOLATILE attribute | F2003 | ✅ | Accepted |  |
| [Statement labels](language-support.md#statement-labels) | F77 or earlier | 🟡 | Labels are accepted; no statement can branch to them except READ `END=` | test_statement_labels.py |
| [Construct and procedure endings](language-support.md#procedure-and-module-endings) | Mixed | ✅ | Bare END: F77 or earlier; construct-specific and named endings: F90. Including a bare `END` for a module | test_unnamed_procedure_ends.py, test_syntax_forms.py |
| COMMON blocks | F77 or earlier | ❌ |  |  |
| [DATA statements](language-support.md#other-statement-forms) | F77 or earlier | 🟡 | Whole variables only, not array elements, sections or implied-DO lists | test_statement_forms_io.py |
| EQUIVALENCE | F77 or earlier | ❌ |  |  |
| Unnamed ENUM, BIND(C) enumerators (Fortran 2003) | F2003 | ❌ |  |  |
| [Named enum types (Fortran 2023)](fortran2023.md#enum-types-section-93) | F2023 | 🟡 | Scalars, arrays, constructors, procedures and integer editing; formatted READ requires constant formats and bounded input lists; C calls remain unsupported | test_enum_types.py, test_enum_arrays.py, test_enum_io.py, test_formatted_read_lists.py |
| Source locations in diagnostics | N/A | ✅ |  | test_source_diagnostics.py |

## Types and kinds

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| [INTEGER kinds 1, 2, 4, 8](language-support.md#numeric-kinds-and-expression-semantics) | F90 | ✅ | F90 kind syntax; the numeric kind values are processor-dependent. | test_integer_literals.py |
| INTEGER kind 16 | F90 | 🟡 | F90 kind syntax; 128-bit integer availability is processor-dependent. Declarations and arithmetic compile; kind-16 literals, HUGE and pointers are not supported |  |
| REAL kinds 4 and 8, DOUBLE PRECISION | Mixed | ✅ | DOUBLE PRECISION: F77 or earlier; kind selection: F90. | test_numeric_model.py |
| REAL kind 16 (quad precision) | F90 | ❌ | F90 kind syntax; quad precision is optional. The REAL128 named constant is F2008. `REAL128` is -1 |  |
| [COMPLEX kinds 4 and 8](language-support.md#complex) | F90 | 🟡 | COMPLEX itself predates F90; explicit kind selection is F90. List-directed and bounded formatted input; input sections and COMPLEX components of derived types remain unsupported | test_complex.py, test_list_read.py, test_formatted_read_lists.py |
| LOGICAL | F77 or earlier | ✅ | Default kind |  |
| [CHARACTER](language-support.md#character-declarations-and-lengths) | F77 or earlier | 🟡 | Lengths must be known at compile time or bounded; see the CHARACTER section | test_character_lengths.py |
| [`SELECTED_INT_KIND`, `SELECTED_REAL_KIND`, `SELECTED_CHAR_KIND`, `KIND`](language-support.md#declaration-initialization), and kind expressions such as `real(kind(0d0))` | Mixed | ✅ | KIND, SELECTED_INT_KIND and SELECTED_REAL_KIND: F90; SELECTED_CHAR_KIND: F2003. A CHARACTER kind other than ASCII is accepted but stored as ASCII | test_initializers_and_kinds.py |
| [ISO_FORTRAN_ENV](language-support.md#intrinsic-modules) | F2003 | 🟡 | Module: F2003; named kind constants such as INT64 and REAL128: F2008. No `ATOMIC_*`, `LOCK_TYPE`, `EVENT_TYPE` or `STAT_*` entities | test_intrinsic_env.py, test_intrinsic_rewrites.py |
| ISO_C_BINDING | F2003 | ❌ |  |  |
| IEEE_ARITHMETIC and other IEEE modules | F2003 | ❌ |  |  |

## Control flow

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| IF, ELSE IF, ELSE, single-line IF | F77 or earlier | ✅ |  | test_else_if.py |
| [DO with step, DO WHILE, unbounded DO](language-support.md#do-loops) | Mixed | ✅ | Counted DO with a step: F77 or earlier; DO WHILE and unbounded DO: F90. The step must be constant | test_do_steps.py |
| [Named loops, EXIT and CYCLE](language-support.md#do-loops) | F90 | ✅ |  | test_named_loops.py |
| [DO CONCURRENT](language-support.md#do-loops) | Mixed | 🟡 | DO CONCURRENT: F2008; locality specifiers: F2018; REDUCE: F2023. Sequential; F2023 REDUCE for scalar and fixed-size explicit-shape intrinsic arrays; no other locality specifiers or type specification | test_statement_forms_io.py, test_concurrent_reduce.py |
| [Labelled DO (`do 10 i = ...`)](language-support.md#do-loops) | F77 or earlier | ✅ |  | test_statement_forms_io.py |
| [SELECT CASE](language-support.md#select-case-ranges) | F90 | 🟡 | INTEGER values and ranges, CHARACTER values; CHARACTER ranges are not supported | test_case_ranges.py |
| BLOCK construct | F2008 | ✅ |  | testsuite, test_named_constructs.py |
| [EXIT from a named BLOCK, IF, SELECT CASE or ASSOCIATE](language-support.md#other-statement-forms) | F2008 | ✅ |  | test_named_constructs.py |
| [ASSOCIATE](language-support.md#associate) | F2003 | 🟡 | Selectors whose value the block changes are rejected | test_associate.py |
| SELECT TYPE, SELECT RANK | Mixed | ❌ | SELECT TYPE: F2003; SELECT RANK: F2018. |  |
| [GOTO](language-support.md#goto) | F77 or earlier | 🟡 | Forward to a label in the same or an enclosing statement list, or backward to a label earlier in the same list; not into a construct | test_goto_and_real_output.py |
| Computed GOTO, assigned GOTO, arithmetic IF | F77 or earlier | ❌ |  |  |
| STOP with a code | Mixed | ✅ | Literal codes: F77 or earlier; scalar constant expressions: F2008; nonconstant expressions: F2018. | test_pure_error_stop.py |
| [ERROR STOP](language-support.md#error-stop) | F2008 | ✅ | ERROR STOP: F2008; permission in PURE procedures: F2018. Also in PURE procedures | test_pure_error_stop.py |

## Arrays

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| Explicit-shape arrays, ranks 1–15 | Mixed | ✅ | Explicit-shape arrays: F77 or earlier; maximum rank increased from 7 to 15 in F2008. | test_vector_ranks.py |
| [Explicit lower bounds](language-support.md#explicit-array-bounds) | F77 or earlier | ✅ | Constant bounds | test_explicit_bounds.py |
| Assumed-shape dummies `x(:)` | F90 | ✅ |  | test_function_arrays.py |
| Assumed-size dummies `x(*)` | F77 or earlier | ❌ | Only for PARAMETER arrays |  |
| [Automatic arrays `w(n)`](language-support.md#runtime-allocation) | F90 | ✅ | Numeric, LOGICAL, fixed-length CHARACTER and derived type; no substring writes to CHARACTER elements | test_automatic_arrays.py |
| [Array constructors and implied-DO](language-support.md#constructors) | F90 | ✅ | Parenthesized (/ ... /) constructors: F90; square brackets: F2003. | test_array_constructors.py, test_runtime_constructors.py |
| [Typed and empty constructors](language-support.md#constructors) | F2003 | ✅ |  | test_empty_constructors.py |
| Sections with constant bounds | F90 | ✅ |  |  |
| [Sections with runtime bounds](language-support.md#runtime-sections) | F90 | ✅ |  | test_runtime_sections.py |
| [Vector subscripts](language-support.md#vector-subscripts) | F90 | ✅ | Not in READ or WHERE | test_vector_subscripts.py |
| Whole-array arithmetic and elemental intrinsics | F90 | ✅ |  |  |
| [WHERE, ELSEWHERE, nested WHERE](language-support.md#where-and-logical-arrays) | Mixed | 🟡 | WHERE and unmasked ELSEWHERE: F90; nesting and masked ELSEWHERE: F95. Section and CHARACTER destinations are not supported | test_where.py, test_nested_where.py |
| [FORALL](language-support.md#forall) | F95 | ✅ | One assignment per FORALL | test_multi_forall.py, test_forall_functions.py |
| [Empty arrays](language-support.md#empty-arrays) | F90 | ✅ |  | test_empty_matrices.py |
| `SIZE`, `SHAPE`, `LBOUND`, `UBOUND` | F90 | ✅ |  | test_bound_inquiries.py |

## ALLOCATABLE and POINTER

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| [ALLOCATABLE arrays with runtime bounds](language-support.md#runtime-allocation) | F90 | ✅ | Numeric and LOGICAL; CHARACTER arrays use bounded storage | test_runtime_allocatables.py, test_heap_allocations.py |
| [ALLOCATABLE dummy arguments](language-support.md#allocatable-dummy-arguments) | F2003 | 🟡 | Subroutines only, not functions | test_allocatable_dummies.py |
| [ALLOCATABLE function results](language-support.md#allocatable-function-results) | F2003 | 🟡 | Assigning a result to an unallocated ALLOCATABLE array is rejected | test_numeric_allocatable_results.py |
| ALLOCATABLE scalars | F2003 | ❌ |  |  |
| [Reallocation on assignment](language-support.md#reallocation-on-assignment) | F2003 | 🟡 | CHARACTER arrays rank one only; a multidimensional expression cannot mix fixed-size and runtime-sized arrays | test_allocatable_reallocation.py, test_runtime_allocatables.py |
| [MOVE_ALLOC](language-support.md#move_alloc-and-allocate-options) | F2003 | ✅ | Copies the data | test_allocation_options.py |
| [ALLOCATE with SOURCE=, MOLD=, STAT=, ERRMSG=](language-support.md#move_alloc-and-allocate-options) | Mixed | ✅ | STAT: F90; SOURCE and ERRMSG: F2003; MOLD: F2008. | test_allocation_options.py |
| Typed ALLOCATE (`allocate(character(len=5) :: s)`) | F2003 | ❌ |  |  |
| [DEALLOCATE](language-support.md#deallocate) | F90 | ✅ |  | test_deallocate.py, test_syntax_forms.py |
| [POINTER association with TARGET](language-support.md#pointer-association) | Mixed | ✅ | POINTER/TARGET: F90; ranks above 7: F2008. Ranks 0–15, sections, strides | test_pointer_association.py, test_pointer_descriptors.py |
| NULLIFY, `NULL()`, ASSOCIATED with one or two arguments | Mixed | ✅ | NULLIFY and ASSOCIATED: F90; NULL(): F95. | test_associate.py |
| ALLOCATE of a POINTER | F90 | ❌ |  |  |
| Pointer bounds remapping | F2003 | ❌ |  |  |
| CHARACTER and derived-type pointers | F90 | ❌ |  |  |

## CHARACTER

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| [Fixed-length scalars, concatenation, comparison](language-support.md#character-declarations-and-lengths) | F77 or earlier | ✅ |  | test_character_comparisons.py |
| [Substrings](language-support.md#scalar-substrings) | F77 or earlier | ✅ |  | test_scalar_substrings.py |
| [Deferred-length `character(len=:), allocatable` scalars](language-support.md#deferred-length-scalar-character) | F2003 | 🟡 | Main-program and BLOCK locals; a value whose largest length is not known at compile time may have at most 4096 characters | test_scalar_substrings.py, test_kinds_and_deferred_strings.py |
| [Deferred-length CHARACTER arrays](language-support.md#deferred-length-scalar-character) | F2003 | 🟡 | Every assignment and ALLOCATE must give the same constant length | test_kinds_and_deferred_strings.py |
| Automatic CHARACTER lengths `character(len=n)` | F90 | ❌ | Lengths derived from `LEN` of a dummy work | test_dummy_character_lengths.py |
| [Fixed-length CHARACTER arrays](language-support.md#character-arrays) | F77 or earlier | ✅ |  | test_character_arrays.py |
| [CHARACTER functions](language-support.md#scalar-character-functions) | F77 or earlier | 🟡 | Assigning the result one character at a time fails at runtime | test_character_functions.py |
| [String intrinsics](language-support.md#character-named-constant-substrings-and-string-intrinsics) | Mixed | ✅ | LEN and INDEX: F77 or earlier; the other listed procedures: F90, except NEW_LINE: F2003. `LEN`, `LEN_TRIM`, `TRIM`, `ADJUSTL`, `ADJUSTR`, `INDEX`, `SCAN`, `VERIFY`, `REPEAT`, `NEW_LINE` | test_character_intrinsics.py |
| [Character codes](language-support.md#character-codes) | Mixed | ✅ | CHAR and ICHAR: F77; ACHAR and IACHAR: F90. `ACHAR`, `CHAR`, `IACHAR`, `ICHAR` | test_character_codes.py |
| [`LGE`, `LGT`, `LLE`, `LLT`](language-support.md#integer-bit-intrinsics) | F77 or earlier | ✅ |  | test_intrinsic_rewrites.py |

## Derived types

Fortran 2023 [enumeration types](fortran2023.md#enumeration-types-us-21-section-92)
are a separate nominal type facility with initial support. See
`tests/test_enumerations.py` for executable coverage and the linked section for
current limits.

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| Scalar components and default initialization | Mixed | ✅ | Derived types with scalar components: F90; component default initialization: F95. |  |
| [Structure constructors](language-support.md#derived-type-components), including keyword components | Mixed | ✅ | Positional constructors: F90; keyword components and omission of default-initialized components: F2003. | test_derived_type_features.py |
| [Array, ALLOCATABLE and POINTER components](language-support.md#derived-type-components) | Mixed | ✅ | Array and POINTER components: F90; ALLOCATABLE components: F2003. | test_derived_array_components.py |
| [Nested derived-type components](language-support.md#derived-type-components) | F90 | ✅ | Not ALLOCATABLE or POINTER scalars | test_derived_array_components.py |
| [Components that are arrays of a derived type](language-support.md#derived-type-components), fixed or ALLOCATABLE | Mixed | ✅ | Fixed-size derived-type array components: F90; ALLOCATABLE components: F2003. The element type must have scalar intrinsic components; no constructors of the containing type | test_derived_type_features.py |
| ALLOCATABLE scalar and deferred-length CHARACTER components | F2003 | ❌ |  |  |
| [Arrays of derived type](language-support.md#arrays-of-derived-type), including elements as procedure arguments and in type-bound calls | Mixed | 🟡 | Arrays of derived type: F90; type-bound calls: F2003. Components may be scalars, fixed-size arrays or derived scalars, not ALLOCATABLE or POINTER | test_allocated_array_functions.py, test_derived_type_features.py |
| Type-bound procedures | F2003 | ✅ |  |  |
| [Generic type-bound procedures, operators and assignment (`generic ::`)](language-support.md#type-bound-procedures-and-generics) | F2003 | ✅ |  | test_derived_type_features.py |
| FINAL procedures | F2003 | ❌ |  |  |
| [Defined operators](language-support.md#defined-operators-with-array-operands) | F90 | ✅ |  | test_defined_operators.py, test_array_operators.py |
| [Defined assignment (`interface assignment(=)`)](language-support.md#type-bound-procedures-and-generics) | F90 | ✅ | To a whole variable, not to an element or component | test_derived_type_features.py |
| [Functions declared with a derived-type prefix (`type(t) function f()`)](language-support.md#other-statement-forms) | F90 | ✅ |  | test_syntax_forms.py |
| [Output of a whole derived object (`print *, obj`)](language-support.md#derived-type-components) | F90 | ✅ | Not with ALLOCATABLE or POINTER components, which need defined I/O | test_derived_type_features.py |
| [Derived-type named constants, SAVE and initialized objects](language-support.md#derived-type-components) | F90 | ✅ | Scalar components | test_derived_type_features.py |
| [Type extension (EXTENDS)](language-support.md#type-extension) | F2003 | 🟡 | Without dynamic dispatch: a binding called through a CLASS dummy cannot be overridden | test_derived_type_features.py |
| CLASS(*), SELECT TYPE, polymorphic ALLOCATABLE objects | F2003 | ❌ |  |  |
| ABSTRACT types | F2003 | ❌ |  |  |
| Parameterized derived types | F2003 | ❌ |  |  |
| Procedure-pointer components | F2003 | ❌ |  |  |

## Procedures

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| Internal and module procedures | F90 | ✅ |  | test_internal_procedures.py, test_module_scope.py |
| External procedures | F77 or earlier | ✅ |  |  |
| [Modules or procedures after the main program in the same file](language-support.md#other-statement-forms) | Mixed | ✅ | External program units: F77 or earlier; modules: F90. Source-file layout is a compiler matter. | test_syntax_forms.py |
| [Calls without an argument list (`call s`, `subroutine s`)](language-support.md#other-statement-forms) | F77 or earlier | ✅ |  | test_syntax_forms.py |
| [Named (keyword) arguments](language-support.md#named-arguments) | F90 | ✅ |  | test_named_arguments.py |
| [OPTIONAL and PRESENT](language-support.md#optional-arguments) | F90 | ✅ |  | test_optional_procedures.py |
| [PURE and ELEMENTAL](language-support.md#elemental-functions) | Mixed | ✅ | PURE/ELEMENTAL: F95; IMPURE ELEMENTAL: F2008. | test_character_elemental.py, test_impure_elemental.py |
| [RECURSIVE](language-support.md#recursive-procedures) | F90 | 🟡 | Recursive functions with array dummies are not supported | test_recursive_arrays.py |
| [Internal procedures with host association](language-support.md#internal-procedures-and-host-association) | F90 | 🟡 | Cannot capture CHARACTER, POINTER, ALLOCATABLE or OPTIONAL host objects, or use whole host arrays | test_internal_procedures.py |
| Generic interfaces | F90 | ✅ |  | test_generic_inquiries.py |
| Interface blocks for external procedures | F90 | ✅ |  |  |
| [Dummy procedures](language-support.md#dummy-procedures-and-procedure-pointers) | F77 or earlier | 🟡 | External dummy procedures: F77 or earlier; explicit interfaces: F90. Intrinsic procedures cannot be passed | test_procedure_arguments.py |
| [Procedure pointers](language-support.md#dummy-procedures-and-procedure-pointers) | F2003 | 🟡 | Not as components or arguments | test_procedure_arguments.py |
| ENTRY statements | F77 or earlier | ❌ |  |  |
| Statement functions | F77 or earlier | ❌ |  |  |

## Modules

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| [USE, ONLY, renaming, PUBLIC and PRIVATE](language-support.md#module-data-and-procedure-association) | F90 | ✅ |  | test_module_scope.py |
| [Separate compilation with `.rmod` interfaces](usage.md#multiple-source-files-and-modules) | F90 | ✅ | Modules and their interfaces: F90; .rmod is rfort-specific. Procedures with dummy procedures must be compiled with their callers | test_module_compilation.py |
| Submodules | F2008 | ❌ |  |  |

## Input and output

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| [List-directed output](language-support.md#list-directed-output) | F77 or earlier | ✅ | REAL values are printed with the shortest exact digits, not gfortran's fixed width | test_logical_list_output.py |
| [Formatted output: A, I, F, L, X, `/`, `:`, I0, F0.d](language-support.md#formatted-output) | Mixed | ✅ | Core descriptors: F77 or earlier; zero-width I0 and F0.d output: F95. | test_write.py, test_runtime_formats.py, test_formatted_output.py |
| [E, ES, EN, D and G edit descriptors, including Ew.dEe, G0 and G0.d](language-support.md#formatted-output) | Mixed | ✅ | E/D/G: F77 or earlier; ES/EN: F90; G0/G0.d: F2008; G0.d for INTEGER/LOGICAL/CHARACTER: F2018. | test_formatted_output.py |
| [B, O, Z edit descriptors](language-support.md#formatted-output) | F90 | ✅ | In the interpreter, a negative value of a non-default INTEGER kind uses the default kind's bits | test_formatted_output.py |
| [T, TL, TR, SP, SS, S, BN, BZ, kP edit descriptors](language-support.md#formatted-output) | F77 or earlier | ✅ | A record with T or TL editing is limited to 4096 characters | test_formatted_output.py |
| [Format reversion](language-support.md#formatted-output) | F77 or earlier | ✅ |  | test_formatted_output.py |
| DC decimal comma; RU, RD, RZ, RN, RC, RP rounding modes | F2003 | ❌ |  |  |
| [FORMAT statements](language-support.md#formatted-output) | F77 or earlier | ✅ |  | test_statement_forms_io.py |
| [Nonadvancing output](language-support.md#formatted-output) | F90 | ✅ |  | test_nonadvancing_output.py |
| [Internal WRITE to a CHARACTER variable](language-support.md#formatted-output) | Mixed | 🟡 | Internal formatted files: F77; list-directed internal I/O: F90. One record; CHARACTER array internal files are not supported. List-directed internal WRITE is supported | test_internal_write.py, test_formatted_output.py |
| [List-directed READ](language-support.md#file-and-standard-input) | Mixed | ✅ | External list-directed input: F77; internal list-directed input: F90. Standard input, files and internal files | test_list_read.py |
| [Formatted READ](language-support.md#file-and-standard-input) | Mixed | 🟡 | Basic formatted input and implied-DOs: F77 or earlier; array sections and B/O/Z editing: F90. Constant formats with scalar, fixed-array, section and bounded implied-DO destinations; runtime formats support only (A); runtime-sized destinations/bounds remain unsupported | test_formatted_read_lists.py, test_enum_io.py, test_runtime_read_format.py, test_statement_forms_io.py |
| [OPEN, CLOSE, REWIND, BACKSPACE for sequential formatted files](language-support.md#file-and-standard-input) | F77 or earlier | 🟡 | IOSTAT is set on OPEN failure; other failed file statements stop the program. STATUS='new' does not check that the file is absent | test_file_io.py, test_formatted_output.py, test_statement_forms_io.py |
| [List-directed WRITE to a file, and WRITE to unit 0 (standard error)](language-support.md#file-and-standard-input) | F77 or earlier | ✅ | List-directed file output: F77; unit 0 for standard error is a processor convention. Items are written as G0 separated by blanks | test_formatted_output.py |
| [INQUIRE](language-support.md#file-and-standard-input) | F77 or earlier | 🟡 | UNIT/FILE with OPENED, NUMBER, NAME, IOSTAT; not EXIST | test_inquire.py |
| Unformatted, stream and direct access | Mixed | ❌ | Sequential/direct unformatted access: F77 or earlier; stream access: F2003. |  |
| NAMELIST | F90 | ❌ |  |  |
| [Defined derived-type I/O](language-support.md#file-and-standard-input) | F2003 | ❌ | Defined derived-type I/O: F2003; default component-wise derived-type I/O: F90 (covered above). |  |

## Intrinsic procedures

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| Elemental math: `SQRT`, `EXP`, `LOG`, `LOG10`, trigonometric and hyperbolic functions | F77 or earlier | ✅ | Basic scalar forms: F77 or earlier; elemental array use: F90; additional COMPLEX trigonometric/hyperbolic forms: F2008. | test_print_array_expressions.py |
| Inverse hyperbolic functions `ASINH`, `ACOSH`, `ATANH` | F2008 | ✅ |  | test_intrinsic_rewrites.py |
| [Degree and half-revolution trigonometry](language-support.md#elemental-math-functions) (`SIND`, `ACOSD`, `ATAN2D`, `SINPI`, `ATAN2PI`, ...) and `ATAN(Y, X)` | Mixed | ✅ | Degree/half-revolution intrinsics: F2023; two-argument ATAN: F2008. DOUBLE PRECISION results can differ from gfortran's in the last bit, and `COSD(90d0)` is 6e-17 instead of 0 | test_intrinsic_rewrites.py |
| `ABS`, `MOD`, `MODULO`, `MIN`, `MAX`, `INT`, `NINT`, `REAL`, `DBLE`, `FLOOR`, `CEILING` | Mixed | 🟡 | Basic arithmetic/conversions: F77 or earlier; MODULO, FLOOR, CEILING and kind arguments: F90. `INT` accepts KIND only for a constant INTEGER argument | test_rounding.py, test_numeric_minmax.py |
| `AINT`, `ANINT`, and `INT` with a KIND argument | Mixed | ✅ | AINT/ANINT: F77; kind arguments: F90. | test_intrinsic_rewrites.py, test_kinds_and_deferred_strings.py |
| `SIGN`, `DIM`, `DPROD` | F77 or earlier | ✅ |  | test_formatted_output.py |
| `ERF`, `ERFC`, `GAMMA`, `LOG_GAMMA`, `HYPOT` | F2008 | ✅ |  | test_intrinsic_rewrites.py |
| [Bessel functions](language-support.md#elemental-math-functions) `BESSEL_J0`, `BESSEL_J1`, `BESSEL_JN`, `BESSEL_Y0`, `BESSEL_Y1`, `BESSEL_YN` | F2008 | 🟡 | Not the transformational `BESSEL_JN(N1, N2, X)` form; REAL(4) results are computed in DOUBLE PRECISION | test_more_intrinsics.py |
| `ERFC_SCALED` | F2008 | ❌ |  |  |
| [Specific names](language-support.md#elemental-math-functions) (`IABS`, `FLOAT`, `DSQRT`, `AMOD`, `ISIGN`, `MAX0`, ...) | F77 or earlier | ✅ |  | test_more_intrinsics.py |
| [`OUT_OF_RANGE`](language-support.md#integer-bit-intrinsics) | F2023 | 🟡 | Scalar arguments; ROUND must be a constant | test_more_intrinsics.py |
| [Numeric inquiries](language-support.md#integer-and-real-inquiries) (`HUGE`, `TINY`, `EPSILON`, `DIGITS`, `PRECISION`, `RANGE`, ...) | F90 | ✅ |  | test_integer_huge.py, test_numeric_model.py |
| [Floating-point model](language-support.md#integer-and-real-inquiries) (`FRACTION`, `EXPONENT`, `SCALE`, `NEAREST`, `SPACING`, ...) | F90 | ✅ |  | test_float_model.py |
| [Bit intrinsics](language-support.md#integer-bit-intrinsics) (`BTEST`, `IAND`, `IOR`, `IEOR`, `NOT`, `IBSET`, `IBCLR`, `IBITS`, `ISHFT`) | F90 | ✅ | Array positions and shifts must be constants or constructors of constants | test_bit_intrinsics.py, test_kinds_and_deferred_strings.py |
| [`ISHFTC`](language-support.md#integer-bit-intrinsics) | F90 | ✅ | Not in PARAMETER initializers | test_intrinsic_rewrites.py |
| [`BGE`, `BGT`, `BLE`, `BLT`](language-support.md#integer-bit-intrinsics) | F2008 | ✅ |  | test_syntax_forms.py |
| [`POPCNT`, `POPPAR`, `LEADZ`, `TRAILZ`, `DSHIFTL`, `DSHIFTR`, `BIT_SIZE`](language-support.md#integer-bit-intrinsics) | Mixed | ✅ | BIT_SIZE: F90; the other listed procedures: F2008. | test_more_intrinsics.py |
| `SHIFTL`, `SHIFTR`, `SHIFTA`, `MASKL`, `MASKR`, `MERGE_BITS` | F2008 | ❌ |  |  |
| [Reductions](language-support.md#reductions) (`SUM`, `PRODUCT`, `MAXVAL`, `MINVAL`, `COUNT`, `ANY`, `ALL`) | F90 | ✅ | DIM must be constant | test_masked_reductions.py, test_dim_reductions.py |
| [`IANY`, `IALL`, `IPARITY`, `PARITY`](language-support.md#reductions) | F2008 | ✅ | DIM must be constant | test_more_intrinsics.py |
| [Locations](language-support.md#array-transformations-and-locations) (`MAXLOC`, `MINLOC`, `FINDLOC`) | Mixed | ✅ | MAXLOC/MINLOC: F90; FINDLOC: F2008. Runtime-sized ARRAY must be rank one | test_locations.py |
| [Array transformations](language-support.md#array-transformations-and-locations) (`PACK`, `UNPACK`, `SPREAD`, `RESHAPE`, `TRANSPOSE`, `CSHIFT`, `EOSHIFT`) | F90 | ✅ | DIM and NCOPIES must be constant | test_pack_unpack.py, test_spread.py, test_reshape_options.py |
| `MERGE` | F90 | 🟡 | Array sources must be numeric or LOGICAL, of constant size | test_formatted_output.py |
| [`DOT_PRODUCT`](language-support.md#dot_product) | F90 | ✅ |  | test_dot_product.py |
| [`MATMUL`](language-support.md#allocatable-arrays-and-pointers) | F90 | 🟡 | Not LOGICAL; a runtime-sized operand must be a whole array, an elemental expression of whole arrays, or a TRANSPOSE or RESHAPE of one | test_complex.py, test_formatted_output.py, test_runtime_matmul.py |
| [`NORM2`](language-support.md#reductions) | F2008 | ✅ | Computed as `SQRT(SUM(X*X))`, without scaling against overflow | test_more_intrinsics.py |
| `TRANSFER`, `STORAGE_SIZE`, `IS_CONTIGUOUS` | Mixed | ❌ | TRANSFER: F90; STORAGE_SIZE and IS_CONTIGUOUS: F2008. |  |
| `RANDOM_NUMBER`, [`RANDOM_SEED`](language-support.md#intrinsic-keywords-and-random_seed) | F90 | ✅ |  | test_intrinsic_keywords.py |
| [`CPU_TIME`, `SYSTEM_CLOCK`, `DATE_AND_TIME`](language-support.md#cpu_time-system_clock-and-date_and_time) | Mixed | ✅ | SYSTEM_CLOCK and DATE_AND_TIME: F90; CPU_TIME: F95. | test_timing.py, test_date_and_time.py |
| [`GET_COMMAND_ARGUMENT`, `GET_ENVIRONMENT_VARIABLE`](language-support.md#command-line-and-environment) | F2003 | ✅ |  | test_command_argument.py, test_environment.py |
| `COMMAND_ARGUMENT_COUNT`, `GET_COMMAND`, `EXECUTE_COMMAND_LINE` | Mixed | ❌ | COMMAND_ARGUMENT_COUNT and GET_COMMAND: F2003; EXECUTE_COMMAND_LINE: F2008. |  |

## Parallel features

| Feature | Introduced | Status | Notes | Tests |
|---|---|---|---|---|
| Coarrays | F2008 | ❌ |  |  |
| OpenMP and OpenACC directives | N/A | ❌ | Separate OpenMP/OpenACC specifications, not Fortran language revisions. Treated as comments |  |

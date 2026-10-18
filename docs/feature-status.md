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

| Feature | Status | Notes | Tests |
|---|---|---|---|
| Free source form (`.f90`, `.f95`, `.f03`, `.f08`) | ✅ | | testsuite |
| Fixed source form (`.f`, `.for`) | ❌ | | |
| `INCLUDE` lines | ❌ | | |
| Implicit typing | ✅ | IMPLICIT COMPLEX is not supported | |
| `IMPLICIT NONE` | ✅ | | |
| [`IMPLICIT NONE (TYPE, EXTERNAL)`](language-support.md#other-statement-forms) | ✅ | EXTERNAL is accepted but not checked | test_syntax_forms.py |
| [DIMENSION statement](language-support.md#other-statement-forms) | ✅ | | test_syntax_forms.py |
| [CHARACTER literals continued across lines](language-support.md#other-statement-forms) | ✅ | | test_syntax_forms.py |
| [Empty or comment-only source files](language-support.md#other-statement-forms) | ✅ | | test_syntax_forms.py |
| [One-word keywords (`ENDIF`, `ELSEIF`, `SELECTCASE`, `DOUBLEPRECISION`, ...)](language-support.md#other-statement-forms) | ✅ | | test_syntax_forms.py |
| [BOZ literal constants](language-support.md#other-statement-forms) | 🟡 | A pattern over 32 bits must be converted with `INT(..., 8)` | test_syntax_forms.py |
| [`ENDFILE` and `FLUSH` statements](language-support.md#file-and-standard-input) | ✅ | | test_statement_forms_io.py |
| [Named IF, SELECT CASE, BLOCK and ASSOCIATE constructs](language-support.md#other-statement-forms) | ✅ | Names after END, ELSE and CASE are not checked against the construct name | test_named_constructs.py |
| [Case-insensitive names](language-support.md#other-statement-forms) | ✅ | | test_syntax_forms.py |
| [Declarations without `::`](language-support.md#declarations-without-) | ✅ | Attributes and initializers still need `::` | test_declarations_without_colons.py |
| [DIMENSION attribute](language-support.md#dimension-declarations-and-shape) | ✅ | | test_dimension_attribute.py |
| [INTENT](language-support.md#intent-spellings) | ✅ | | |
| [PARAMETER attribute and statement](language-support.md#parameter-statements) | ✅ | Derived-type named constants need scalar components | test_parameter_statements.py, test_named_constants.py, test_derived_type_features.py |
| [Declaration initialization](language-support.md#declaration-initialization) | 🟡 | Saved arrays are not supported | test_main_initialization.py, test_saved_locals.py, test_initializers_and_kinds.py |
| SAVE | 🟡 | Scalars only | test_saved_locals.py |
| VALUE attribute | ✅ | | |
| PROTECTED attribute | ❌ | | |
| CONTIGUOUS attribute | ✅ | Accepted | |
| VOLATILE attribute | ✅ | Accepted | |
| [Statement labels](language-support.md#statement-labels) | 🟡 | Labels are accepted; no statement can branch to them except READ `END=` | test_statement_labels.py |
| [Construct and procedure endings](language-support.md#procedure-and-module-endings) | ✅ | Including a bare `END` for a module | test_unnamed_procedure_ends.py, test_syntax_forms.py |
| COMMON blocks | ❌ | | |
| [DATA statements](language-support.md#other-statement-forms) | 🟡 | Whole variables only, not array elements, sections or implied-DO lists | test_statement_forms_io.py |
| EQUIVALENCE | ❌ | | |
| ENUM, BIND(C) enumerators | ❌ | | |
| Source locations in diagnostics | ✅ | | test_source_diagnostics.py |

## Types and kinds

| Feature | Status | Notes | Tests |
|---|---|---|---|
| [INTEGER kinds 1, 2, 4, 8](language-support.md#numeric-kinds-and-expression-semantics) | ✅ | | test_integer_literals.py |
| INTEGER kind 16 | 🟡 | Declarations and arithmetic compile; kind-16 literals, HUGE and pointers are not supported | |
| REAL kinds 4 and 8, DOUBLE PRECISION | ✅ | | test_numeric_model.py |
| REAL kind 16 (quad precision) | ❌ | `REAL128` is -1 | |
| [COMPLEX kinds 4 and 8](language-support.md#complex) | 🟡 | COMPLEX input and COMPLEX components of derived types are not supported | test_complex.py |
| LOGICAL | ✅ | Default kind | |
| [CHARACTER](language-support.md#character-declarations-and-lengths) | 🟡 | Lengths must be known at compile time or bounded; see the CHARACTER section | test_character_lengths.py |
| [`SELECTED_INT_KIND`, `SELECTED_REAL_KIND`, `SELECTED_CHAR_KIND`, `KIND`](language-support.md#declaration-initialization), and kind expressions such as `real(kind(0d0))` | ✅ | A CHARACTER kind other than ASCII is accepted but stored as ASCII | test_initializers_and_kinds.py |
| [ISO_FORTRAN_ENV](language-support.md#intrinsic-modules) | 🟡 | No `ATOMIC_*`, `LOCK_TYPE`, `EVENT_TYPE` or `STAT_*` entities | test_intrinsic_env.py, test_intrinsic_rewrites.py |
| ISO_C_BINDING | ❌ | | |
| IEEE_ARITHMETIC and other IEEE modules | ❌ | | |

## Control flow

| Feature | Status | Notes | Tests |
|---|---|---|---|
| IF, ELSE IF, ELSE, single-line IF | ✅ | | test_else_if.py |
| [DO with step, DO WHILE, unbounded DO](language-support.md#do-loops) | ✅ | The step must be constant | test_do_steps.py |
| [Named loops, EXIT and CYCLE](language-support.md#do-loops) | ✅ | | test_named_loops.py |
| [DO CONCURRENT](language-support.md#do-loops) | 🟡 | Runs its iterations in order; no type specification or locality specifiers | test_statement_forms_io.py |
| [Labelled DO (`do 10 i = ...`)](language-support.md#do-loops) | ✅ | | test_statement_forms_io.py |
| [SELECT CASE](language-support.md#select-case-ranges) | 🟡 | INTEGER values and ranges, CHARACTER values; CHARACTER ranges are not supported | test_case_ranges.py |
| BLOCK construct | ✅ | | testsuite, test_named_constructs.py |
| [EXIT from a named BLOCK, IF, SELECT CASE or ASSOCIATE](language-support.md#other-statement-forms) | ✅ | | test_named_constructs.py |
| [ASSOCIATE](language-support.md#associate) | 🟡 | Selectors whose value the block changes are rejected | test_associate.py |
| SELECT TYPE, SELECT RANK | ❌ | | |
| GOTO, computed GOTO, arithmetic IF | ❌ | | |
| STOP with a code | ✅ | | test_pure_error_stop.py |
| [ERROR STOP](language-support.md#error-stop) | ✅ | Also in PURE procedures | test_pure_error_stop.py |

## Arrays

| Feature | Status | Notes | Tests |
|---|---|---|---|
| Explicit-shape arrays, ranks 1–15 | ✅ | | test_vector_ranks.py |
| [Explicit lower bounds](language-support.md#explicit-array-bounds) | ✅ | Constant bounds | test_explicit_bounds.py |
| Assumed-shape dummies `x(:)` | ✅ | | test_function_arrays.py |
| Assumed-size dummies `x(*)` | ❌ | Only for PARAMETER arrays | |
| [Automatic arrays `w(n)`](language-support.md#runtime-allocation) | ✅ | Numeric, LOGICAL, fixed-length CHARACTER and derived type; no substring writes to CHARACTER elements | test_automatic_arrays.py |
| [Array constructors and implied-DO](language-support.md#constructors) | ✅ | | test_array_constructors.py, test_runtime_constructors.py |
| [Typed and empty constructors](language-support.md#constructors) | ✅ | | test_empty_constructors.py |
| Sections with constant bounds | ✅ | | |
| [Sections with runtime bounds](language-support.md#runtime-sections) | ✅ | | test_runtime_sections.py |
| [Vector subscripts](language-support.md#vector-subscripts) | ✅ | Not in READ or WHERE | test_vector_subscripts.py |
| Whole-array arithmetic and elemental intrinsics | ✅ | | |
| [WHERE, ELSEWHERE, nested WHERE](language-support.md#where-and-logical-arrays) | 🟡 | Section and CHARACTER destinations are not supported | test_where.py, test_nested_where.py |
| [FORALL](language-support.md#forall) | ✅ | One assignment per FORALL | test_multi_forall.py, test_forall_functions.py |
| [Empty arrays](language-support.md#empty-arrays) | ✅ | | test_empty_matrices.py |
| `SIZE`, `SHAPE`, `LBOUND`, `UBOUND` | ✅ | | test_bound_inquiries.py |

## ALLOCATABLE and POINTER

| Feature | Status | Notes | Tests |
|---|---|---|---|
| [ALLOCATABLE arrays with runtime bounds](language-support.md#runtime-allocation) | ✅ | Numeric and LOGICAL; CHARACTER arrays use bounded storage | test_runtime_allocatables.py, test_heap_allocations.py |
| [ALLOCATABLE dummy arguments](language-support.md#allocatable-dummy-arguments) | 🟡 | Subroutines only, not functions | test_allocatable_dummies.py |
| [ALLOCATABLE function results](language-support.md#allocatable-function-results) | 🟡 | Assigning a result to an unallocated ALLOCATABLE array is rejected | test_numeric_allocatable_results.py |
| ALLOCATABLE scalars | ❌ | | |
| [Reallocation on assignment](language-support.md#reallocation-on-assignment) | 🟡 | Not for CHARACTER arrays; a multidimensional expression cannot mix fixed-size and runtime-sized arrays | test_allocatable_reallocation.py, test_runtime_allocatables.py |
| [MOVE_ALLOC](language-support.md#move_alloc-and-allocate-options) | ✅ | Copies the data | test_allocation_options.py |
| [ALLOCATE with SOURCE=, MOLD=, STAT=, ERRMSG=](language-support.md#move_alloc-and-allocate-options) | ✅ | | test_allocation_options.py |
| Typed ALLOCATE (`allocate(character(len=5) :: s)`) | ❌ | | |
| [DEALLOCATE](language-support.md#deallocate) | ✅ | | test_deallocate.py, test_syntax_forms.py |
| [POINTER association with TARGET](language-support.md#pointer-association) | ✅ | Ranks 0–15, sections, strides | test_pointer_association.py, test_pointer_descriptors.py |
| NULLIFY, `NULL()`, ASSOCIATED with one or two arguments | ✅ | | test_associate.py |
| ALLOCATE of a POINTER | ❌ | | |
| Pointer bounds remapping | ❌ | | |
| CHARACTER and derived-type pointers | ❌ | | |

## CHARACTER

| Feature | Status | Notes | Tests |
|---|---|---|---|
| [Fixed-length scalars, concatenation, comparison](language-support.md#character-declarations-and-lengths) | ✅ | | test_character_comparisons.py |
| [Substrings](language-support.md#scalar-substrings) | ✅ | | test_scalar_substrings.py |
| [Deferred-length `character(len=:), allocatable` scalars](language-support.md#deferred-length-scalar-character) | 🟡 | Main-program and BLOCK locals; a value whose largest length is not known at compile time may have at most 4096 characters | test_scalar_substrings.py, test_kinds_and_deferred_strings.py |
| [Deferred-length CHARACTER arrays](language-support.md#deferred-length-scalar-character) | 🟡 | Every assignment and ALLOCATE must give the same constant length | test_kinds_and_deferred_strings.py |
| Automatic CHARACTER lengths `character(len=n)` | ❌ | Lengths derived from `LEN` of a dummy work | test_dummy_character_lengths.py |
| [Fixed-length CHARACTER arrays](language-support.md#character-arrays) | ✅ | | test_character_arrays.py |
| [CHARACTER functions](language-support.md#scalar-character-functions) | 🟡 | Assigning the result one character at a time fails at runtime | test_character_functions.py |
| [String intrinsics](language-support.md#character-named-constant-substrings-and-string-intrinsics) | ✅ | `LEN`, `LEN_TRIM`, `TRIM`, `ADJUSTL`, `ADJUSTR`, `INDEX`, `SCAN`, `VERIFY`, `REPEAT`, `NEW_LINE` | test_character_intrinsics.py |
| [Character codes](language-support.md#character-codes) | ✅ | `ACHAR`, `CHAR`, `IACHAR`, `ICHAR` | test_character_codes.py |
| [`LGE`, `LGT`, `LLE`, `LLT`](language-support.md#integer-bit-intrinsics) | ✅ | | test_intrinsic_rewrites.py |

## Derived types

| Feature | Status | Notes | Tests |
|---|---|---|---|
| Scalar components and default initialization | ✅ | | |
| [Structure constructors](language-support.md#derived-type-components), including keyword components | ✅ | | test_derived_type_features.py |
| [Array, ALLOCATABLE and POINTER components](language-support.md#derived-type-components) | ✅ | | test_derived_array_components.py |
| [Nested derived-type components](language-support.md#derived-type-components) | ✅ | Not ALLOCATABLE or POINTER scalars | test_derived_array_components.py |
| [Components that are arrays of a derived type](language-support.md#derived-type-components), fixed or ALLOCATABLE | ✅ | The element type must have scalar intrinsic components; no constructors of the containing type | test_derived_type_features.py |
| ALLOCATABLE scalar and deferred-length CHARACTER components | ❌ | | |
| [Arrays of derived type](language-support.md#arrays-of-derived-type), including elements as procedure arguments and in type-bound calls | 🟡 | Components may be scalars, fixed-size arrays or derived scalars, not ALLOCATABLE or POINTER | test_allocated_array_functions.py, test_derived_type_features.py |
| Type-bound procedures | ✅ | | |
| [Generic type-bound procedures, operators and assignment (`generic ::`)](language-support.md#type-bound-procedures-and-generics) | ✅ | | test_derived_type_features.py |
| FINAL procedures | ❌ | | |
| [Defined operators](language-support.md#defined-operators-with-array-operands) | ✅ | | test_defined_operators.py, test_array_operators.py |
| [Defined assignment (`interface assignment(=)`)](language-support.md#type-bound-procedures-and-generics) | ✅ | To a whole variable, not to an element or component | test_derived_type_features.py |
| [Functions declared with a derived-type prefix (`type(t) function f()`)](language-support.md#other-statement-forms) | ✅ | | test_syntax_forms.py |
| [Output of a whole derived object (`print *, obj`)](language-support.md#derived-type-components) | ✅ | Not with ALLOCATABLE or POINTER components, which need defined I/O | test_derived_type_features.py |
| [Derived-type named constants, SAVE and initialized objects](language-support.md#derived-type-components) | ✅ | Scalar components | test_derived_type_features.py |
| [Type extension (EXTENDS)](language-support.md#type-extension) | 🟡 | Without dynamic dispatch: a binding called through a CLASS dummy cannot be overridden | test_derived_type_features.py |
| CLASS(*), SELECT TYPE, polymorphic ALLOCATABLE objects | ❌ | | |
| ABSTRACT types | ❌ | | |
| Parameterized derived types | ❌ | | |
| Procedure-pointer components | ❌ | | |

## Procedures

| Feature | Status | Notes | Tests |
|---|---|---|---|
| Internal and module procedures | ✅ | | test_internal_procedures.py, test_module_scope.py |
| External procedures | ✅ | | |
| [Modules or procedures after the main program in the same file](language-support.md#other-statement-forms) | ✅ | | test_syntax_forms.py |
| [Calls without an argument list (`call s`, `subroutine s`)](language-support.md#other-statement-forms) | ✅ | | test_syntax_forms.py |
| [Named (keyword) arguments](language-support.md#named-arguments) | ✅ | | test_named_arguments.py |
| [OPTIONAL and PRESENT](language-support.md#optional-arguments) | ✅ | | test_optional_procedures.py |
| [PURE and ELEMENTAL](language-support.md#elemental-functions) | ✅ | | test_character_elemental.py, test_impure_elemental.py |
| [RECURSIVE](language-support.md#recursive-procedures) | 🟡 | Recursive functions with array dummies are not supported | test_recursive_arrays.py |
| [Internal procedures with host association](language-support.md#internal-procedures-and-host-association) | 🟡 | Cannot capture CHARACTER, POINTER, ALLOCATABLE or OPTIONAL host objects, or use whole host arrays | test_internal_procedures.py |
| Generic interfaces | ✅ | | test_generic_inquiries.py |
| Interface blocks for external procedures | ✅ | | |
| [Dummy procedures](language-support.md#dummy-procedures-and-procedure-pointers) | 🟡 | Intrinsic procedures cannot be passed | test_procedure_arguments.py |
| [Procedure pointers](language-support.md#dummy-procedures-and-procedure-pointers) | 🟡 | Not as components or arguments | test_procedure_arguments.py |
| ENTRY statements | ❌ | | |
| Statement functions | ❌ | | |

## Modules

| Feature | Status | Notes | Tests |
|---|---|---|---|
| [USE, ONLY, renaming, PUBLIC and PRIVATE](language-support.md#module-data-and-procedure-association) | ✅ | | test_module_scope.py |
| [Separate compilation with `.rmod` interfaces](usage.md#multiple-source-files-and-modules) | ✅ | Procedures with dummy procedures must be compiled with their callers | test_module_compilation.py |
| Submodules | ❌ | | |

## Input and output

| Feature | Status | Notes | Tests |
|---|---|---|---|
| [List-directed output](language-support.md#list-directed-output) | ✅ | REAL values are printed with the shortest exact digits, not gfortran's fixed width | test_logical_list_output.py |
| [Formatted output: A, I, F, L, X, `/`, `:`, I0, F0.d](language-support.md#formatted-output) | ✅ | | test_write.py, test_runtime_formats.py, test_formatted_output.py |
| [E, ES, EN, D and G edit descriptors, including Ew.dEe, G0 and G0.d](language-support.md#formatted-output) | ✅ | | test_formatted_output.py |
| [B, O, Z edit descriptors](language-support.md#formatted-output) | ✅ | In the interpreter, a negative value of a non-default INTEGER kind uses the default kind's bits | test_formatted_output.py |
| [T, TL, TR, SP, SS, S, BN, BZ, kP edit descriptors](language-support.md#formatted-output) | ✅ | A record with T or TL editing is limited to 4096 characters | test_formatted_output.py |
| [Format reversion](language-support.md#formatted-output) | ✅ | | test_formatted_output.py |
| DC decimal comma; RU, RD, RZ, RN, RC, RP rounding modes | ❌ | | |
| [FORMAT statements](language-support.md#formatted-output) | ✅ | | test_statement_forms_io.py |
| [Nonadvancing output](language-support.md#formatted-output) | ✅ | | test_nonadvancing_output.py |
| [Internal WRITE to a CHARACTER variable](language-support.md#formatted-output) | 🟡 | One record; CHARACTER array internal files are not supported. List-directed internal WRITE is supported | test_internal_write.py, test_formatted_output.py |
| [List-directed READ](language-support.md#file-and-standard-input) | ✅ | Standard input, files and internal files | test_list_read.py |
| [Formatted READ](language-support.md#file-and-standard-input) | 🟡 | A constant format of A, I, F, E, D, G, L, X, T and `/` into scalars, elements and constant-size arrays; not B, O or Z, implied-DO lists or runtime-sized arrays | test_runtime_read_format.py, test_statement_forms_io.py |
| [OPEN, CLOSE, REWIND, BACKSPACE for sequential formatted files](language-support.md#file-and-standard-input) | 🟡 | IOSTAT is set on OPEN failure; other failed file statements stop the program. STATUS='new' does not check that the file is absent | test_file_io.py, test_formatted_output.py, test_statement_forms_io.py |
| [List-directed WRITE to a file, and WRITE to unit 0 (standard error)](language-support.md#file-and-standard-input) | ✅ | Items are written as G0 separated by blanks | test_formatted_output.py |
| [INQUIRE](language-support.md#file-and-standard-input) | 🟡 | UNIT/FILE with OPENED, NUMBER, NAME, IOSTAT; not EXIST | test_inquire.py |
| Unformatted, stream and direct access | ❌ | | |
| NAMELIST | ❌ | | |
| Derived-type I/O | ❌ | | |

## Intrinsic procedures

| Feature | Status | Notes | Tests |
|---|---|---|---|
| Elemental math: `SQRT`, `EXP`, `LOG`, `LOG10`, trigonometric and hyperbolic functions | ✅ | | test_print_array_expressions.py |
| Inverse hyperbolic functions `ASINH`, `ACOSH`, `ATANH` | ✅ | | test_intrinsic_rewrites.py |
| [Degree and half-revolution trigonometry](language-support.md#elemental-math-functions) (`SIND`, `ACOSD`, `ATAN2D`, `SINPI`, `ATAN2PI`, ...) and `ATAN(Y, X)` | ✅ | DOUBLE PRECISION results can differ from gfortran's in the last bit, and `COSD(90d0)` is 6e-17 instead of 0 | test_intrinsic_rewrites.py |
| `ABS`, `MOD`, `MODULO`, `MIN`, `MAX`, `INT`, `NINT`, `REAL`, `DBLE`, `FLOOR`, `CEILING` | 🟡 | `INT` accepts KIND only for a constant INTEGER argument | test_rounding.py, test_numeric_minmax.py |
| `AINT`, `ANINT`, and `INT` with a KIND argument | ✅ | | test_intrinsic_rewrites.py, test_kinds_and_deferred_strings.py |
| `SIGN`, `DIM`, `DPROD` | ✅ | | test_formatted_output.py |
| `ERF`, `ERFC`, `GAMMA`, `LOG_GAMMA`, `HYPOT` | ✅ | | test_intrinsic_rewrites.py |
| [Bessel functions](language-support.md#elemental-math-functions) `BESSEL_J0`, `BESSEL_J1`, `BESSEL_JN`, `BESSEL_Y0`, `BESSEL_Y1`, `BESSEL_YN` | 🟡 | Not the transformational `BESSEL_JN(N1, N2, X)` form; REAL(4) results are computed in DOUBLE PRECISION | test_more_intrinsics.py |
| `ERFC_SCALED` | ❌ | | |
| [Specific names](language-support.md#elemental-math-functions) (`IABS`, `FLOAT`, `DSQRT`, `AMOD`, `ISIGN`, `MAX0`, ...) | ✅ | | test_more_intrinsics.py |
| [`OUT_OF_RANGE`](language-support.md#integer-bit-intrinsics) | 🟡 | Scalar arguments; ROUND must be a constant | test_more_intrinsics.py |
| [Numeric inquiries](language-support.md#integer-and-real-inquiries) (`HUGE`, `TINY`, `EPSILON`, `DIGITS`, `PRECISION`, `RANGE`, ...) | ✅ | | test_integer_huge.py, test_numeric_model.py |
| [Floating-point model](language-support.md#integer-and-real-inquiries) (`FRACTION`, `EXPONENT`, `SCALE`, `NEAREST`, `SPACING`, ...) | ✅ | | test_float_model.py |
| [Bit intrinsics](language-support.md#integer-bit-intrinsics) (`BTEST`, `IAND`, `IOR`, `IEOR`, `NOT`, `IBSET`, `IBCLR`, `IBITS`, `ISHFT`) | ✅ | Array positions and shifts must be constants or constructors of constants | test_bit_intrinsics.py, test_kinds_and_deferred_strings.py |
| [`ISHFTC`](language-support.md#integer-bit-intrinsics) | ✅ | Not in PARAMETER initializers | test_intrinsic_rewrites.py |
| [`BGE`, `BGT`, `BLE`, `BLT`](language-support.md#integer-bit-intrinsics) | ✅ | | test_syntax_forms.py |
| [`POPCNT`, `POPPAR`, `LEADZ`, `TRAILZ`, `DSHIFTL`, `DSHIFTR`, `BIT_SIZE`](language-support.md#integer-bit-intrinsics) | ✅ | | test_more_intrinsics.py |
| `SHIFTL`, `SHIFTR`, `SHIFTA`, `MASKL`, `MASKR`, `MERGE_BITS` | ❌ | | |
| [Reductions](language-support.md#reductions) (`SUM`, `PRODUCT`, `MAXVAL`, `MINVAL`, `COUNT`, `ANY`, `ALL`) | ✅ | DIM must be constant | test_masked_reductions.py, test_dim_reductions.py |
| [`IANY`, `IALL`, `IPARITY`, `PARITY`](language-support.md#reductions) | ✅ | DIM must be constant | test_more_intrinsics.py |
| [Locations](language-support.md#array-transformations-and-locations) (`MAXLOC`, `MINLOC`, `FINDLOC`) | ✅ | | test_locations.py |
| [Array transformations](language-support.md#array-transformations-and-locations) (`PACK`, `UNPACK`, `SPREAD`, `RESHAPE`, `TRANSPOSE`, `CSHIFT`, `EOSHIFT`) | ✅ | DIM and NCOPIES must be constant | test_pack_unpack.py, test_spread.py, test_reshape_options.py |
| `MERGE` | 🟡 | Array sources must be numeric or LOGICAL, of constant size | test_formatted_output.py |
| [`DOT_PRODUCT`](language-support.md#dot_product) | ✅ | | test_dot_product.py |
| [`MATMUL`](language-support.md#allocatable-arrays-and-pointers) | 🟡 | Not LOGICAL; a runtime-sized operand must be a whole array, an elemental expression of whole arrays, or a TRANSPOSE or RESHAPE of one | test_complex.py, test_formatted_output.py, test_runtime_matmul.py |
| [`NORM2`](language-support.md#reductions) | ✅ | Computed as `SQRT(SUM(X*X))`, without scaling against overflow | test_more_intrinsics.py |
| `TRANSFER`, `STORAGE_SIZE`, `IS_CONTIGUOUS` | ❌ | | |
| `RANDOM_NUMBER`, [`RANDOM_SEED`](language-support.md#intrinsic-keywords-and-random_seed) | ✅ | | test_intrinsic_keywords.py |
| [`CPU_TIME`, `SYSTEM_CLOCK`, `DATE_AND_TIME`](language-support.md#cpu_time-system_clock-and-date_and_time) | ✅ | | test_timing.py, test_date_and_time.py |
| [`GET_COMMAND_ARGUMENT`, `GET_ENVIRONMENT_VARIABLE`](language-support.md#command-line-and-environment) | ✅ | | test_command_argument.py, test_environment.py |
| `COMMAND_ARGUMENT_COUNT`, `GET_COMMAND`, `EXECUTE_COMMAND_LINE` | ❌ | | |

## Parallel features

| Feature | Status | Notes | Tests |
|---|---|---|---|
| Coarrays | ❌ | | |
| OpenMP and OpenACC directives | ❌ | Treated as comments | |

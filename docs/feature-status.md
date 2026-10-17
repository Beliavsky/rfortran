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
with rfort and comparing their output with gfortran. Rows marked ✅ or 🟡
name the tests that cover them. Update this page when a feature changes.

## Known wrong results

These programs compile without error but produce wrong output. They are bugs,
not unsupported features.

| Feature | Problem |
|---|---|
| Automatic arrays | `real :: w(n)` in a procedure, where `n` is a dummy argument, gives the wrong size and wrong element values. |
| `LGE`, `LGT`, `LLE`, `LLT` | Return false (native) or print nothing (interpreter) instead of the comparison result. |
| `ISHFTC` | Returns 0. |

## Source form and declarations

| Feature | Status | Notes | Tests |
|---|---|---|---|
| Free source form (`.f90`, `.f95`, `.f03`, `.f08`) | ✅ | | testsuite |
| Fixed source form (`.f`, `.for`) | ❌ | | |
| `INCLUDE` lines | ❌ | | |
| Implicit typing | ✅ | IMPLICIT COMPLEX is not supported | |
| `IMPLICIT NONE` | ✅ | | |
| [Declarations without `::`](language-support.md#declarations-without-) | ✅ | Attributes and initializers still need `::` | test_declarations_without_colons.py |
| [DIMENSION attribute](language-support.md#dimension-declarations-and-shape) | ✅ | | test_dimension_attribute.py |
| [INTENT](language-support.md#intent-spellings) | ✅ | | |
| [PARAMETER attribute and statement](language-support.md#parameter-statements) | ✅ | Derived-type named constants are not supported | test_parameter_statements.py, test_named_constants.py |
| [Declaration initialization](language-support.md#declaration-initialization) | 🟡 | Saved arrays are not supported | test_main_initialization.py, test_saved_locals.py |
| SAVE | 🟡 | Scalars only | test_saved_locals.py |
| VALUE attribute | ✅ | | |
| PROTECTED attribute | ❌ | | |
| CONTIGUOUS attribute | ✅ | Accepted | |
| VOLATILE attribute | ✅ | Accepted | |
| [Statement labels](language-support.md#statement-labels) | 🟡 | Labels are accepted; no statement can branch to them except READ `END=` | test_statement_labels.py |
| [Construct and procedure endings](language-support.md#procedure-and-module-endings) | ✅ | | test_unnamed_procedure_ends.py |
| COMMON blocks | ❌ | | |
| DATA statements | ❌ | | |
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
| `SELECTED_INT_KIND`, `SELECTED_REAL_KIND`, `KIND` | ✅ | | |
| [ISO_FORTRAN_ENV kinds](language-support.md#intrinsic-modules) | 🟡 | Kind constants only | test_intrinsic_env.py |
| ISO_C_BINDING | ❌ | | |
| IEEE_ARITHMETIC and other IEEE modules | ❌ | | |

## Control flow

| Feature | Status | Notes | Tests |
|---|---|---|---|
| IF, ELSE IF, ELSE, single-line IF | ✅ | | test_else_if.py |
| [DO with step, DO WHILE, unbounded DO](language-support.md#do-loops) | ✅ | The step must be constant | test_do_steps.py |
| [Named loops, EXIT and CYCLE](language-support.md#do-loops) | ✅ | | test_named_loops.py |
| DO CONCURRENT | ❌ | | |
| Labelled DO (`do 10 i = ...`) | ❌ | | |
| [SELECT CASE](language-support.md#select-case-ranges) | 🟡 | INTEGER values and ranges, CHARACTER values; CHARACTER ranges are not supported | test_case_ranges.py |
| BLOCK construct | ✅ | | testsuite |
| EXIT from a named BLOCK | ❌ | | |
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
| Automatic arrays `w(n)` | ⚠️ | Wrong results; see above | |
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
| [Reallocation on assignment](language-support.md#reallocation-on-assignment) | 🟡 | Not for CHARACTER arrays | test_allocatable_reallocation.py |
| [MOVE_ALLOC](language-support.md#move_alloc-and-allocate-options) | ✅ | Copies the data | test_allocation_options.py |
| [ALLOCATE with SOURCE=, MOLD=, STAT=, ERRMSG=](language-support.md#move_alloc-and-allocate-options) | ✅ | | test_allocation_options.py |
| Typed ALLOCATE (`allocate(character(len=5) :: s)`) | ❌ | | |
| [DEALLOCATE](language-support.md#deallocate) | ✅ | | test_deallocate.py |
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
| [Deferred-length `character(len=:), allocatable` scalars](language-support.md#deferred-length-scalar-character) | 🟡 | Storage must be bounded; `s = s // 'x'` in a loop is rejected | test_scalar_substrings.py |
| Deferred-length CHARACTER arrays | ❌ | | |
| Automatic CHARACTER lengths `character(len=n)` | ❌ | Lengths derived from `LEN` of a dummy work | test_dummy_character_lengths.py |
| [Fixed-length CHARACTER arrays](language-support.md#character-arrays) | ✅ | | test_character_arrays.py |
| [CHARACTER functions](language-support.md#scalar-character-functions) | 🟡 | Assigning the result one character at a time fails at runtime | test_character_functions.py |
| [String intrinsics](language-support.md#character-named-constant-substrings-and-string-intrinsics) | ✅ | `LEN`, `LEN_TRIM`, `TRIM`, `ADJUSTL`, `ADJUSTR`, `INDEX`, `SCAN`, `VERIFY`, `REPEAT`, `NEW_LINE` | test_character_intrinsics.py |
| [Character codes](language-support.md#character-codes) | ✅ | `ACHAR`, `CHAR`, `IACHAR`, `ICHAR` | test_character_codes.py |
| `LGE`, `LGT`, `LLE`, `LLT` | ⚠️ | Wrong results; see above | |

## Derived types

| Feature | Status | Notes | Tests |
|---|---|---|---|
| Scalar components and default initialization | ✅ | | |
| Structure constructors | ✅ | | |
| [Array, ALLOCATABLE and POINTER components](language-support.md#derived-type-components) | ✅ | | test_derived_array_components.py |
| [Nested derived-type components](language-support.md#derived-type-components) | ✅ | Not ALLOCATABLE or POINTER, and not arrays | test_derived_array_components.py |
| ALLOCATABLE scalar and deferred-length CHARACTER components | ❌ | | |
| [Arrays of derived type](language-support.md#arrays-of-derived-type) | 🟡 | Components must be scalars | test_allocated_array_functions.py |
| Type-bound procedures | ✅ | | |
| Generic type-bound procedures (`generic ::`) | ❌ | | |
| FINAL procedures | ❌ | | |
| [Defined operators](language-support.md#defined-operators-with-array-operands) | ✅ | | test_defined_operators.py, test_array_operators.py |
| Defined assignment (`interface assignment(=)`) | ❌ | | |
| Functions declared with a derived-type prefix (`type(t) function f()`) | ❌ | Use a RESULT variable | |
| Printing a whole derived object (`print *, obj`) | ❌ | | |
| Derived-type named constants | ❌ | | |
| Type extension (EXTENDS), CLASS, polymorphism | ❌ | | |
| ABSTRACT types | ❌ | | |
| Parameterized derived types | ❌ | | |
| Procedure-pointer components | ❌ | | |

## Procedures

| Feature | Status | Notes | Tests |
|---|---|---|---|
| Internal and module procedures | ✅ | | test_internal_procedures.py, test_module_scope.py |
| External procedures | ✅ | | |
| Calls without an argument list (`call s`, `subroutine s`) | ❌ | Write `call s()` and `subroutine s()` | |
| [Named (keyword) arguments](language-support.md#named-arguments) | ✅ | | test_named_arguments.py |
| [OPTIONAL and PRESENT](language-support.md#optional-arguments) | ✅ | | test_optional_procedures.py |
| [PURE and ELEMENTAL](language-support.md#elemental-functions) | ✅ | | test_character_elemental.py, test_impure_elemental.py |
| [RECURSIVE](language-support.md#recursive-procedures) | 🟡 | Recursive functions with array dummies are not supported | test_recursive_arrays.py |
| [Internal procedures with host association](language-support.md#internal-procedures-and-host-association) | 🟡 | Cannot capture CHARACTER, POINTER, ALLOCATABLE or OPTIONAL host objects | test_internal_procedures.py |
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
| [Formatted output: A, I, F, L, X, `/`, I0, F0.d, G0](language-support.md#formatted-output) | ✅ | | test_write.py, test_runtime_formats.py |
| E, ES, EN, G, D edit descriptors | ❌ | | |
| B, O, Z edit descriptors | ❌ | | |
| T, TL, TR, SP, SS, BN, BZ, P edit descriptors | ❌ | | |
| FORMAT statements | ❌ | Use a format string | |
| [Nonadvancing output](language-support.md#formatted-output) | ✅ | | test_nonadvancing_output.py |
| [Internal WRITE to a CHARACTER variable](language-support.md#formatted-output) | 🟡 | One record; CHARACTER array internal files are not supported | test_internal_write.py |
| [List-directed READ](language-support.md#file-and-standard-input) | ✅ | Standard input, files and internal files | test_list_read.py |
| [Formatted READ](language-support.md#file-and-standard-input) | 🟡 | Only the `(A)` format | test_runtime_read_format.py |
| [OPEN, CLOSE, REWIND, BACKSPACE for sequential formatted files](language-support.md#file-and-standard-input) | 🟡 | IOSTAT on OPEN and CLOSE, and CLOSE STATUS=, are not supported | test_file_io.py |
| List-directed WRITE to a file | ❌ | Use an explicit format | |
| [INQUIRE](language-support.md#file-and-standard-input) | 🟡 | UNIT/FILE with OPENED, NUMBER, NAME, IOSTAT; not EXIST | test_inquire.py |
| Unformatted, stream and direct access | ❌ | | |
| NAMELIST | ❌ | | |
| Derived-type I/O | ❌ | | |

## Intrinsic procedures

| Feature | Status | Notes | Tests |
|---|---|---|---|
| Elemental math: `SQRT`, `EXP`, `LOG`, `LOG10`, trigonometric and hyperbolic functions | ✅ | | |
| Inverse hyperbolic functions `ASINH`, `ACOSH`, `ATANH` | ❌ | | |
| `ABS`, `MOD`, `MODULO`, `MIN`, `MAX`, `INT`, `NINT`, `REAL`, `DBLE`, `FLOOR`, `CEILING` | ✅ | | test_rounding.py, test_numeric_minmax.py |
| `SIGN`, `DIM`, `AINT`, `ANINT`, `DPROD` | ❌ | | |
| `ERF`, `ERFC`, `GAMMA`, `LOG_GAMMA`, `HYPOT`, Bessel functions | ❌ | | |
| [Numeric inquiries](language-support.md#integer-and-real-inquiries) (`HUGE`, `TINY`, `EPSILON`, `DIGITS`, `PRECISION`, `RANGE`, ...) | ✅ | | test_integer_huge.py, test_numeric_model.py |
| [Floating-point model](language-support.md#integer-and-real-inquiries) (`FRACTION`, `EXPONENT`, `SCALE`, `NEAREST`, `SPACING`, ...) | ✅ | | test_float_model.py |
| [Bit intrinsics](language-support.md#integer-bit-intrinsics) (`BTEST`, `IAND`, `IOR`, `IEOR`, `NOT`, `IBSET`, `IBCLR`, `IBITS`, `ISHFT`) | ✅ | Scalars only | test_bit_intrinsics.py |
| `ISHFTC` | ⚠️ | Wrong results; see above | |
| `POPCNT`, `LEADZ`, `TRAILZ`, `SHIFTL`, `MASKR`, `BIT_SIZE` | ❌ | | |
| [Reductions](language-support.md#reductions) (`SUM`, `PRODUCT`, `MAXVAL`, `MINVAL`, `COUNT`, `ANY`, `ALL`) | ✅ | DIM must be constant | test_masked_reductions.py, test_dim_reductions.py |
| [Locations](language-support.md#array-transformations-and-locations) (`MAXLOC`, `MINLOC`, `FINDLOC`) | ✅ | | test_locations.py |
| [Array transformations](language-support.md#array-transformations-and-locations) (`PACK`, `UNPACK`, `SPREAD`, `RESHAPE`, `TRANSPOSE`, `CSHIFT`, `EOSHIFT`) | ✅ | DIM and NCOPIES must be constant | test_pack_unpack.py, test_spread.py, test_reshape_options.py |
| `MERGE` | 🟡 | Scalars only | |
| [`DOT_PRODUCT`](language-support.md#dot_product) | ✅ | | test_dot_product.py |
| `MATMUL` | 🟡 | COMPLEX arrays only | test_complex.py |
| `NORM2` | ❌ | | |
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

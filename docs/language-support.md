# Language support

This page describes the Fortran features rfort supports, with their
restrictions and the tests that check them. It changes as the compiler
grows; [feature-status.md](feature-status.md) is a one-line-per-feature
summary. Unless noted otherwise, each Python test compares the interpreter
(`rfort run`) and native builds at `-O0` and `-O2`, and compares with
gfortran when it is installed. Run the tests after `cargo build --bin rfort`.

## Contents

- [Source form and declarations](#source-form-and-declarations)
  - [INCLUDE lines](#include-lines)
  - [Preprocessor conditionals](#preprocessor-conditionals)
  - [Declarations without `::`](#declarations-without-)
  - [DIMENSION declarations and SHAPE](#dimension-declarations-and-shape)
  - [INTENT spellings](#intent-spellings)
  - [PARAMETER statements](#parameter-statements)
  - [Declaration initialization](#declaration-initialization)
  - [Contextual keywords](#contextual-keywords)
  - [Procedure and module endings](#procedure-and-module-endings)
  - [Statement labels](#statement-labels)
  - [Declaration diagnostics](#declaration-diagnostics)
- [Numeric types and intrinsics](#numeric-types-and-intrinsics)
  - [Numeric kinds and expression semantics](#numeric-kinds-and-expression-semantics)
  - [Comparisons](#comparisons)
  - [COMPLEX](#complex)
  - [Integer and real inquiries](#integer-and-real-inquiries)
  - [Integer bit intrinsics](#integer-bit-intrinsics)
  - [Rounding, ISNAN, MIN and MAX](#rounding-isnan-min-and-max)
  - [Elemental math functions](#elemental-math-functions)
- [Control flow](#control-flow)
  - [DO loops](#do-loops)
  - [GOTO](#goto)
  - [SELECT CASE ranges](#select-case-ranges)
  - [ASSOCIATE](#associate)
  - [ERROR STOP](#error-stop)
- [CHARACTER](#character)
  - [CHARACTER declarations and lengths](#character-declarations-and-lengths)
  - [CHARACTER named-constant substrings and string intrinsics](#character-named-constant-substrings-and-string-intrinsics)
  - [Scalar substrings](#scalar-substrings)
  - [Deferred-length scalar CHARACTER](#deferred-length-scalar-character)
  - [Character codes](#character-codes)
  - [CHARACTER comparisons and function arguments](#character-comparisons-and-function-arguments)
  - [CHARACTER arrays](#character-arrays)
  - [CHARACTER array parameters](#character-array-parameters)
- [Arrays](#arrays)
  - [Explicit array bounds](#explicit-array-bounds)
  - [Array bounds inquiries](#array-bounds-inquiries)
  - [Empty arrays](#empty-arrays)
  - [Constructors](#constructors)
  - [RESHAPE, MOD and MODULO](#reshape-mod-and-modulo)
  - [Vector subscripts](#vector-subscripts)
  - [Runtime sections](#runtime-sections)
  - [WHERE and LOGICAL arrays](#where-and-logical-arrays)
  - [FORALL](#forall)
  - [Reductions](#reductions)
  - [DOT_PRODUCT](#dot_product)
  - [Array transformations and locations](#array-transformations-and-locations)
- [ALLOCATABLE arrays and pointers](#allocatable-arrays-and-pointers)
  - [Runtime allocation](#runtime-allocation)
  - [ALLOCATABLE dummy arguments](#allocatable-dummy-arguments)
  - [DEALLOCATE](#deallocate)
  - [Reallocation on assignment](#reallocation-on-assignment)
  - [MOVE_ALLOC and ALLOCATE options](#move_alloc-and-allocate-options)
  - [ALLOCATABLE function results](#allocatable-function-results)
  - [Pointer association](#pointer-association)
- [Derived types](#derived-types)
  - [Derived-type components](#derived-type-components)
  - [Arrays of derived type](#arrays-of-derived-type)
  - [Type-bound procedures and generics](#type-bound-procedures-and-generics)
  - [Type extension](#type-extension)
  - [ABSTRACT types and DEFERRED bindings](#abstract-types-and-deferred-bindings)
  - [Scalar polymorphic ALLOCATABLE components](#scalar-polymorphic-allocatable-components)
  - [Scalar polymorphic ALLOCATABLE objects](#scalar-polymorphic-allocatable-objects)
  - [Scalar polymorphic allocatable dummy arguments](#scalar-polymorphic-allocatable-dummy-arguments)
  - [Scalar polymorphic POINTER objects](#scalar-polymorphic-pointer-objects)
  - [Scalar polymorphic POINTER components](#scalar-polymorphic-pointer-components)
  - [Scalar dynamic binding dispatch](#scalar-dynamic-binding-dispatch)
  - [SELECT TYPE](#select-type)
  - [Derived arguments and OPTIONAL](#derived-arguments-and-optional)
- [Procedures](#procedures)
  - [C interoperability](#c-interoperability)
  - [External procedures and separate compilation](#external-procedures-and-separate-compilation)
  - [Named arguments](#named-arguments)
  - [OPTIONAL arguments](#optional-arguments)
  - [CHARACTER arguments](#character-arguments)
  - [Scalar CHARACTER functions](#scalar-character-functions)
  - [CHARACTER array functions](#character-array-functions)
  - [Array arguments to functions](#array-arguments-to-functions)
  - [Forwarded array-valued function calls](#forwarded-array-valued-function-calls)
  - [Defined operators with array operands](#defined-operators-with-array-operands)
  - [Generic and elemental dispatch](#generic-and-elemental-dispatch)
  - [ELEMENTAL functions](#elemental-functions)
  - [ELEMENTAL subroutines and purity](#elemental-subroutines-and-purity)
  - [Dummy procedures and procedure pointers](#dummy-procedures-and-procedure-pointers)
  - [Internal procedures and host association](#internal-procedures-and-host-association)
  - [Recursive procedures](#recursive-procedures)
- [Modules](#modules)
  - [Module data and procedure association](#module-data-and-procedure-association)
  - [Intrinsic modules](#intrinsic-modules)
- [Input and output](#input-and-output)
  - [Formatted output](#formatted-output)
  - [List-directed output](#list-directed-output)
  - [File and standard input](#file-and-standard-input)
- [System, random and timing intrinsics](#system-random-and-timing-intrinsics)
  - [Command line and environment](#command-line-and-environment)
  - [Intrinsic keywords and RANDOM_SEED](#intrinsic-keywords-and-random_seed)
  - [CPU_TIME, SYSTEM_CLOCK and DATE_AND_TIME](#cpu_time-system_clock-and-date_and_time)

## Source form and declarations

### Fixed source form

Files ending in `.f`, `.for`, `.ftn`, `.f77` or `.fpp`, in either case, are read as
fixed source form, and other Fortran files as free form. `-ffixed-form` and
`-ffree-form` override the extension for every source. Fixed-form and
free-form files can be compiled together.

Each fixed-form file is converted to free form as it is read, by the
`fixed2free` converter in `tools/fixed2free`. The conversion keeps every line on
its line number, so errors are reported at the original line. Columns can
differ by a few characters on lines that the conversion changed, for example
where it added a blank in `STOP'done'`.

The conversion follows the fixed-form rules:

- Columns 1-5 hold a label, a non-blank other than `0` in column 6 marks a
  continuation, and text after column 72 is ignored. Use
  `-ffixed-line-length-N` or `-ffixed-line-length-none` to change the last
  column; `-Wall` reports ignored text.
- `C`, `c`, `*` or `!` in column 1 starts a comment line. `D` lines are
  comments unless `-fd-lines-as-code` is given.
- DEC tab format is accepted.
- A CHARACTER literal continued onto the next line keeps its blanks up to the
  last column.
- Blanks inside numbers and operators, such as `.70710 67811` or `.AND .`, are
  removed, and Hollerith constants in FORMAT statements become quoted strings.

Code that relies on blanks being insignificant inside names and keywords, such
as `DO10I=1,5` or `GOTO10`, is not supported. A warning is shown for a construct that the conversion may not
preserve, such as a statement starting before column 7; `-Werror` makes these
warnings errors.

Run `python tests/test_fixed_form.py` for the checks.

### INCLUDE lines

Fortran `INCLUDE 'filename'` and `INCLUDE "filename"` insert source text before
parsing. Includes can contain declarations, executable statements, or program
units; nested and repeated includes are supported. Relative paths are searched
beside the including file, then in the working directory, then in repeatable
`-I directory` search paths. Absolute filenames are also accepted. Included files
inherit the enclosing source's fixed/free form and source-form options, regardless
of their extension. Filenames may use doubled quote delimiters and a trailing
Fortran comment is allowed.

An INCLUDE occupies a complete source line, outside a continued statement.
Cycles, missing files, malformed lines, and nesting beyond 128 files produce
errors. Compile-time errors, warnings, and initialization runtime errors retain
the original filenames and line numbers, including parent lines after an include.
The same handling applies to native builds, `run`, `check`, `parse`, and separate
module compilation.

Preprocessor conditionals are applied to the outer file first, so an INCLUDE in
an excluded branch is ignored. Text inserted by Fortran INCLUDE is not itself
preprocessed. C-preprocessor `#include` remains unsupported.

```fortran
implicit none
include 'constants.inc'
print *, n
end
```

With `integer, parameter :: n = 7` in `constants.inc`, run this using
`rfort main.f90 --run`, or `rfort -I includes main.f90 --run` when the include
file is in `includes`. Run `python tests/test_include.py` for the regression tests.

### Preprocessor conditionals

As with gfortran, sources whose extension is upper case, such as `.F`, `.FOR`,
`.F90` and `.F08`, and `.fpp` files, go through conditional compilation before
anything else; `-cpp` does this for every source and `-nocpp` for none.
`-DNAME` and `-DNAME=VALUE` define names, and a name without a value is 1.

- `#if`, `#ifdef`, `#ifndef`, `#elif`, `#else` and `#endif`, nested.
  Conditions are C integer expressions with `defined NAME` or `defined(NAME)`,
  the arithmetic, comparison, logical, bit and `?:` operators and parentheses;
  a name that is not defined is 0. `_OPENMP` is not defined.
- `#define NAME [VALUE]` and `#undef NAME`, for later conditions.
- `#error`, which stops the compilation with its text.

Directive lines and the lines a condition excludes become blank, so errors are
reported at the original line numbers. Names are not replaced in Fortran
lines: a line that uses a defined name, outside character constants and
comments, is an error. `#include`, `#line`, `#pragma` and function-like
macros are not supported and are errors where a condition includes them.
This covers LAPACK's `.F` files, which test `defined(_OPENMP)`.
Run `python tests/test_preprocessor.py`.

### Declarations without `::`

Intrinsic type declarations may omit `::` when they have no attributes or
initializers. Examples include `character(len=8) ampm`,
`integer(kind=4) values(8)`, and `real x,y(2)`. These forms support the same
type selectors, entity lengths, and array bounds as declarations with `::`.
Attributes such as `PARAMETER` or `INTENT`, and declaration initialization,
still require `::`.

Run `python tests/test_declarations_without_colons.py` for interpreter/native
O0/O2 checks with gfortran comparisons.

### DIMENSION declarations and SHAPE

Intrinsic, `TYPE(...)`, `CLASS(...)`, `TYPEOF(...)`, and `CLASSOF(...)`
declarations accept `DIMENSION(...)` alongside attributes such
as `INTENT`, `OPTIONAL`, `ALLOCATABLE`, `PARAMETER`, and `SAVE`. Bounds apply to
each entity; an entity's own array declarator overrides the attribute's bounds.
For example, `integer, dimension(2,3) :: a,b` declares two matrices, and
`integer, dimension(:), intent(in) :: nn` declares an assumed-shape argument.
These forms use the same supported array operations as per-entity bounds.
For example, `type(point), dimension(2) :: x,y(3)` is equivalent to
`type(point) :: x(2),y(3)`. Recognition is contextual: an entity or procedure
named `dimension` remains an ordinary identifier.

Parser comparisons cover intrinsic and derived types, polymorphic and
referenced types, ranks through 15, explicit and runtime bounds, assumed/deferred
shape, assumed size and assumed rank, both attribute orders, and declarations in
main programs, modules, procedures and derived-type components. Existing
semantic/backend limits on particular type/shape combinations apply equally to
both forms; recognizing a CLASS array declaration does not add general
polymorphic-array execution support. Labeled specification statements retain
their existing parser limitation.

`SHAPE` supports fixed-size numeric/LOGICAL array variables and sections,
specialized array arguments, and supported runtime allocatable arrays. Fixed-size inquiries do
not read the array's elements, so the array need not be initialized.
The optional result `KIND` currently supports only default INTEGER (`KIND=4`);
`SOURCE=` and `KIND=` keywords are accepted. Run
`python tests/test_dimension_attribute.py` for interpreter/native O0/O2 checks
with gfortran comparisons.

### INTENT spellings

`intent(inout)` and `intent(in out)` are equivalent declaration attributes.
Both spellings use the same argument checks and propagate updates to the caller.
They are case insensitive, and `in` / `out` may be separated by a valid
free-form continuation. `intent(in)` and `intent(out)` retain their usual meanings.

### PARAMETER statements

Separate `PARAMETER (name=value, ...)` statements are supported for explicitly
declared scalar constants. For example, `integer :: i, j` followed by
`parameter (i=5, j=i+2)` defines the same constants as a modern PARAMETER
attribute declaration. The type declaration must precede the statement; values
use the existing numeric/CHARACTER constant evaluation and conversion rules.
Module exports, host association, PURE procedures and assignment protection
are preserved. Implicitly typed constants and array names in separate PARAMETER
statements remain unsupported. Run `python tests/test_parameter_statements.py`.

### Declaration initialization

Main-program variables support declaration initialization from supported constant
expressions. Numeric scalars, array constructors (including constant
implied-DOs), scalar array fills, supported LOGICAL constant expressions and fixed-length CHARACTER
scalars are supported. Arrays of any rank may be initialized with RESHAPE of a
constant constructor, with SHAPE given as a constructor or as `SHAPE(array)` of
the declared array, and with PAD and ORDER. Values remain mutable; initialization
runs once before normal use, including declarations with an explicit SAVE
attribute. INTEGER initializers currently require integer constant expressions,
which may use `ABS`, `MOD`, `MODULO`, `SIGN`, `DIM`, `MAX`, `MIN` and `KIND`.
Other constant array transformations remain outside this subset. Run
`python tests/test_main_initialization.py` and
`python tests/test_initializers_and_kinds.py`.

A kind type parameter may be any constant INTEGER expression, such as
`real(kind(0.0d0))`, `integer(kind=selected_int_kind(9))` or a named constant.
`KIND(x)` of a declared variable, including a COMPLEX part `z%re`, is a
constant, so `cmplx(x, y, kind=kind(z))` works. `SELECTED_CHAR_KIND('ASCII')`
is 1, and `CHARACTER(LEN=n, KIND=k)` is accepted; CHARACTER data is always
stored as ASCII, whatever the kind.

Initialized INTEGER, REAL, DOUBLE PRECISION, LOGICAL and fixed-length CHARACTER
scalar locals in functions
and subroutines now have persistent SAVE storage, including locals declared inside
procedure BLOCK scopes. Declaration initialization implies SAVE even when the
attribute is omitted. Values survive repeated calls, OPTIONAL specializations and
recursive activations; each declaration has independent storage. Ordinary locals
without initialization retain their existing behavior. PURE procedures reject
saved initialized locals. The SAVE attribute and the SAVE statement also save
scalars without initializers; SAVE without a list saves every local scalar of
the procedure. Local arrays of these types are saved the same way, by the SAVE
attribute or statement or by a declaration initializer, and use static storage
like module arrays; a saved array without an initializer starts undefined.
Saved CHARACTER arrays are limited to 1024 elements. TARGET locals and module
initialization remain unsupported.
Run `python tests/test_saved_locals.py` and `python tests/test_saved_characters.py`;
`tests/programs/xsave_counter.f90` is a small numeric example.

### Contextual keywords

DOUBLE and PRECISION are contextual words: they form a type declaration when
paired within one logical statement, including continued lines. Otherwise they
can name variables, arrays, arguments or procedures. Run
`python tests/test_double_precision_names.py`.

### Procedure and module endings

Subroutines and functions accept a bare `END`, an unnamed `END SUBROUTINE`
or `END FUNCTION`, or an ending with the procedure name. Modules accept
`END MODULE` with or without their name. An omitted name does not consume the
following source statement, including across comments and semicolons;
continuations within an ending statement are supported.

Run `python tests/test_unnamed_procedure_ends.py` for interpreter/native O0/O2
checks with gfortran comparisons.

### Other statement forms

- `call s` and `subroutine s` may omit an empty argument list, and a module may
  end with a bare `END`.
- `DATA a / 1 /, b, c / 2*0.5, 3.0 /` gives whole variables initial values,
  with `r*c` repeat counts (`r` may be a named constant), signed and BOZ
  constants. The variables must be declared before the DATA statement; an
  array must be the last variable of its value list. Array elements with
  constant subscripts and implied-DO lists, as in
  `data ((mm(i,j), j = 1, 4), i = 1, 2) / ... /`, may give an array its
  values element by element, if every element gets one; bounds may be named
  constants. Sections are not yet supported. A local array of a procedure
  that is given values by DATA and never changed, such as a table of pivots,
  is a named constant; one that the procedure changes is not yet supported,
  as it would need SAVE storage.
- `EQUIVALENCE (cr(1,1), crv(1)), (a, b)` makes each later object of a set
  share the storage of the first, so `crv(k)` is the k-th element of `cr` in
  column-major order. The objects must be local variables of the same type,
  with constant bounds, and each later object must lie within the first. A
  whole later array, or a section of it, cannot yet be used.
- `INTRINSIC REAL, MAX` and `EXTERNAL` lists may name procedures spelled like
  keywords.
- The FORTRAN 77 sized types `REAL*4`, `REAL*8`, `COMPLEX*8`, `COMPLEX*16`,
  `INTEGER*n` and `LOGICAL*n`, `DOUBLE COMPLEX`, and the specific COMPLEX
  intrinsics `DIMAG`, `DCONJG`, `DREAL` and `DCMPLX` are accepted.
- Statement functions, such as `cabs1(z) = abs(dble(z)) + abs(dimag(z))`, are
  expanded where they are referenced, with any argument types, COMPLEX
  included, and the result converted to the function's declared type.
- An assignment converts between INTEGER, REAL and DOUBLE PRECISION; a real
  value assigned to an INTEGER variable is truncated toward zero, as by INT.
- A local array may have the name of an intrinsic, such as `sum(2,2)`; its
  references are elements, not the intrinsic.
- Comment and blank lines may appear between the continued lines of a
  character literal, and a name or number may be split across lines with `&`
  at the end of one line and the start of the next.
- SELECT and CASE may be used as variable names, and SELECT as an array name,
  as in `select(k) = .false.`. IF may name a variable, as in `if = i - 1`; it
  is a keyword only before a parenthesized condition and after END or ELSE.
- External procedures and modules may follow the main program in the same file.
- `IMPLICIT NONE (TYPE)`, `IMPLICIT NONE (EXTERNAL)` and
  `IMPLICIT NONE (TYPE, EXTERNAL)` are accepted. TYPE disables implicit
  typing; EXTERNAL is accepted but not checked.
- A function may be declared `type(t) function f(...)`, with or without RESULT.
- The DIMENSION statement (`dimension a(2), b(0:n)`) gives array bounds to
  entities typed elsewhere in the same scope or typed implicitly.
- A CHARACTER literal may continue onto the next line with `&` at the end of
  the line; the next line's leading `&` is optional, as in gfortran.
- A source file may be empty or contain only comments.
- Two-word keywords may be written as one word: `ENDIF`, `ELSEIF`, `ENDDO`,
  `ENDSELECT`, `SELECTCASE`, `ENDBLOCK`, `ENDASSOCIATE`, `ENDFORALL`,
  `ENDFUNCTION`, `ENDSUBROUTINE`, `ENDPROGRAM`, `ENDMODULE`, `ENDINTERFACE`,
  `ENDTYPE` and `DOUBLEPRECISION`. A variable with one of these names is still
  a variable when it is assigned or referenced.
- BOZ literal constants (`b'0101'`, `o"17"`, `z'ff'`) are INTEGER values: the
  bits as a default INTEGER when they fit in 32 bits, otherwise as INTEGER(8).
  `INT(boz, KIND)` with a constant KIND keeps that kind's low-order bits.
- A relational or logical operator may follow an integer literal without a
  blank, as in `i>=97.and.i<=122`.
- Names are case-insensitive: `Count`, `COUNT` and `count` are the same
  variable.
- IF, SELECT CASE, BLOCK and ASSOCIATE constructs may be named
  (`search: block ... end block search`). `EXIT name` leaves any of them, and an
  EXIT or CYCLE without a name still refers to the innermost DO loop. The
  optional names after END, ELSE and CASE are accepted without checking that
  they match.
- Variables declared in a BLOCK are local to it, even when an outer variable
  has the same name.

Run `python tests/test_syntax_forms.py` and
`python tests/test_named_constructs.py`.

### Statement labels

Executable statements may have numeric labels of one to five digits, with
values from 1 through 99999. Labels are local to a program unit, so different
procedures may reuse them; duplicates within a unit and undefined I/O branch
targets are diagnosed. `CONTINUE` is a no-op, and remains available as an
identifier outside statement context. Labelled output, input, assignments,
single-line IF statements, and supported file operations use the same behavior
as their unlabelled forms.

Labels do not yet enable GOTO, labelled DO termination, FORMAT statements, or
I/O control transfers through `ERR=` and `EOR=`. READ supports the forward
`END=` transfers described under [File and standard input](#file-and-standard-input). Unsupported controls
are rejected rather than ignored. Run `python tests/test_statement_labels.py`
for interpreter/native O0/O2 checks with gfortran comparisons.

### Declaration diagnostics

Unsupported ELEMENTAL declarations and errors while resolving types or
constant initializers now report the source filename, line, and column and
underline the relevant attribute, name, or statement. For example, an unsupported
POINTER ELEMENTAL function dummy is diagnosed at its declaration, naming the argument
and procedure. The same reporting is used by native builds, `check`, and `run`.

Native multi-source builds retain explicit source ranges so these diagnostics
refer to the original file and line instead of `main.f90c-sources.f90`.
Declaration source offsets remain available through AST transformations;
passes that have not yet attached source locations retain their existing text
errors. Run `python tests/test_source_diagnostics.py` to check CLI modes,
continuations, repeated names, type errors, and original-file mapping.

## Numeric types and intrinsics

### Numeric kinds and expression semantics

Integer literals accept numeric or named kind suffixes, such as `127_1` and
`10000000000_int64`, for kinds 1, 2, 4 and 8. The compiler checks literal ranges
and preserves their kinds in supported expressions, array constructors and
procedure arguments. KIND supports numeric arithmetic expressions without
reading their operands; mixed INTEGER operands select the wider kind. A REAL
base raised to an INTEGER power retains the base kind. Exponentiation binds
more tightly than unary signs and associates to the right: `-2**2` is -4,
`(-2)**2` is 4, and `2**3**2` is 512. INTEGER PARAMETER declarations support
kinds 1, 2, 4 and 8, preserving kind and checking the declared range.
Kind-16 integer literals remain unsupported. Run
`python tests/test_integer_literals.py`.

Named constants support INTEGER, REAL (kinds 4 and 8), DOUBLE PRECISION,
default LOGICAL and CHARACTER scalars and rank-one arrays. LOGICAL initializers
support literals, previously defined constants, `.not.`, `.and.`, `.or.`,
`.eqv.`, `.neqv.`, scalar numeric/CHARACTER comparisons, and ALL/ANY of logical
constructors or named constant arrays (including empty arrays). CHARACTER
comparisons pad the shorter operand with spaces; INTEGER comparisons retain
integer precision. Module imports, host association and separate PARAMETER
statements use the same constant resolution. Constant arrays still have the
existing rank-one limits; COMPLEX constants support the subset described below.
Derived-type named constants are not implemented. Run `python tests/test_named_constants.py`.

### Comparisons

INTEGER and REAL operands support `==`, `/=`, `<`, `<=`, `>` and `>=`, and
the dotted spellings `.eq.`, `.ne.`, `.lt.`, `.le.`, `.gt.` and `.ge.`.
CHARACTER uses the same operators with blank padding of shorter operands.
COMPLEX supports equality and inequality; ordered COMPLEX comparisons are
rejected. LOGICAL equivalence uses `.eqv.` and `.neqv.`.

Supported scalar and array forms include scalar broadcasting in either order,
array-to-array comparisons, scalar array-element operands, assumed-shape
dummies, runtime-sized storage, sections and automatic LOGICAL result arrays.
Generated subscript/bound helpers retain their result types during array
expression and output lowering; scalar array elements keep their bounds checks.
Multidimensional CHARACTER copies preserve linear storage coordinates.

Derived types can overload all six relational operators through explicit
interfaces. ELEMENTAL comparison procedures apply to scalar, array and mixed
scalar/array operands, including inside ALL/ANY/COUNT and assignments.

Run `python tests/test_array_comparisons.py` for comparisons against gfortran in
the interpreter and native O0/O2 builds. Coverage includes INTEGER kinds 1/2/4/8,
REAL and COMPLEX kinds 4/8, default LOGICAL and CHARACTER, mixed numeric types,
blank padding, all six derived-type comparisons, empty arrays and ranks through
15. General storage and declaration restrictions elsewhere on this page still
apply. `tests/test_elemental_operators.py` covers additional overloaded operator
cases, including higher-rank derived arrays.

### COMPLEX

COMPLEX uses REAL component kinds 4 (default) and 8. Supported forms include
kind selectors, literals `(real,imaginary)`, `CMPLX` (including keywords),
`%re`/`%im`, scalar and array assignment, constructors and simple implied-DOs,
arithmetic, equality, integer and real/complex powers, `REAL`, `DBLE`, `AIMAG`,
`ABS`, `CONJG`, `SUM`, conjugated `DOT_PRODUCT`, and rank-one/rank-two `MATMUL`.
The implemented complex math functions are `SQRT`, `EXP`, `LOG`, `LOG10`,
`SIN`, `COS`, `TAN`, `SINH`, `COSH`, and `TANH` (`LOG10` with COMPLEX is an
extension). Ordinary complex dummy arguments
and scalar function results use the existing procedure lowering. Complex named
constants support literals, conversion, conjugation and basic arithmetic.

List-directed output prints compact `(real,imaginary)` pairs, trimming generated
buffer padding while retaining ordinary CHARACTER padding. Explicit formats consume
two real descriptors for each complex element. REAL `G0`/`G0.d` output is also
supported. `CMPLX` without KIND returns default COMPLEX; `REAL(z)` and `AIMAG(z)`
retain a complex argument's component kind. Initialization checks apply to each
component. List-directed input supports scalar and whole-array COMPLEX;
constant-format input also supports bounded implied-DO lists, with two real
fields per element. COMPLEX input sections remain unsupported.
Derived types may have COMPLEX scalar and fixed-size array components, also in
nested types and in arrays of records: `p%z`, `p%z%im`, `a(i)%z` and
`aimag(a(i)%z)` work as COMPLEX values, and list-directed output of a whole
object prints its COMPLEX components in parentheses. An ASSOCIATE name whose
selector is a COMPLEX expression supports `r%re` and `r%im`.

This is an initial subset: kind 16, default initialization of COMPLEX components, implicit COMPLEX declarations, persistent initialized
complex locals, and user-defined complex ELEMENTAL procedures are not implemented.
List-directed array output currently requires fixed extents.

Run `python tests/test_complex.py` after building. The seven external probes in
`tests/ofort/complex-selection.json` cover constructors, parts, magnitude,
roots, component kinds, arithmetic, dot products and matrix multiplication:

```bat
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --compiler target/debug/rfort.exe --selection tests/ofort/complex-selection.json --report target/ofort-complex.json --strict
```

### Integer and real inquiries

`HUGE` supports INTEGER kinds 1, 2, 4 and 8. Default-integer literal/constant
expressions such as `huge(0)` fold in PARAMETER initializers; executable inquiries
also accept typed scalar or array variables without reading their values.
`KIND` reports declared integer kinds correctly, including `kind(huge(i8))`.
INTEGER(16) HUGE is diagnosed rather than truncated; TINY still requires REAL.
Run `python tests/test_integer_huge.py` for interpreter/native O0/O2 and gfortran
comparison checks. Executable HUGE/TINY also accept supported numeric expressions
and specialized array views, preserving kind without reading array elements or
requiring nonempty storage. KIND of their results preserves the input kind.
Related numeric-model inquiries also recognize these views. Run
`python tests/test_array_model_inquiries.py` for empty/uninitialized array dummy
and expression checks. Typed variable inquiries inside PARAMETER initializers
remain outside this subset.

`EPSILON`, `DIGITS`, `RADIX`, `PRECISION`, `RANGE`, `MINEXPONENT`, and
`MAXEXPONENT` support REAL kinds 4 and 8. `DIGITS`, `RADIX`, and `RANGE` also
support INTEGER kinds 1, 2, 4 and 8. EPSILON retains its argument's real kind;
the other results are default INTEGER scalars, including for array arguments.
The inquiries accept `X=` and work in PARAMETER initializers using supported
constants or previously declared variables. They determine the type without
reading variable values or evaluating array subscripts; an unset variable is
valid here. Supported arguments include variables, whole arrays, array elements,
rank-one sections, arithmetic expressions and scalar REAL/DBLE/INT conversions.
PRECISION and RANGE also accept COMPLEX kinds 4 and 8. General procedure-call
arguments for these inquiries are not implemented.
Variables and user procedures may still use these intrinsic names.

`SCALE`, `FRACTION`, `EXPONENT`, `SET_EXPONENT`, `NEAREST`, `SPACING`, and
`RRSPACING` support REAL kinds 4 and 8, keyword arguments, scalar expansion,
conformable arrays, PURE procedures, and supported constant expressions.
EXPONENT returns default INTEGER; the other results retain X's real kind.
SCALE and SET_EXPONENT accept INTEGER kinds 1/2/4/8 for I. NEAREST's nonzero
REAL direction argument may have a different kind from X; a zero direction
is diagnosed at compile time when constant and otherwise at runtime.
The shared binary implementation handles signed zero, subnormal values, and
scaling across the representable range without an intermediate power overflow.
Run `python tests/test_float_model.py` for interpreter/native O0/O2 and gfortran
comparison checks.

### Integer bit intrinsics

Scalar `BTEST`, `IAND`, `IOR`, `IEOR`, `NOT`, `IBSET`, `IBCLR`, `IBITS`, and
`ISHFT` support INTEGER kinds 1/2/4/8 in expressions, output, conditions, PURE
procedures, and constant initialization. Named arguments are accepted. `BTEST`
returns LOGICAL; the other results retain the first argument's INTEGER kind.
`IAND`, `IOR`, and `IEOR` require both arguments to have the same kind.

Bit positions start at zero. Invalid constant positions, field lengths, and
shift counts are compile errors; dynamic values are checked at runtime.
Zero-length fields and shifts by the full bit width are supported. Initialization
checks apply to the input values. The functions are elemental; with array
arguments, positions, lengths and shift counts must be constants. INTEGER kind
16 is not supported.

`POPCNT`, `POPPAR`, `LEADZ` and `TRAILZ` count bits and return a default
INTEGER. `DSHIFTL(I, J, SHIFT)` and `DSHIFTR(I, J, SHIFT)` shift the bits of I
and J as one double-width value; I and J have the same kind and SHIFT is
between 0 and the bit size, checked at run time. `BIT_SIZE(I)` is a constant of
I's kind. `OUT_OF_RANGE(X, MOLD [, ROUND])` tells whether X can be converted to
MOLD's type and kind: a REAL X is truncated, or rounded when ROUND is true,
before it is compared with an INTEGER range. X and MOLD must be scalars and
ROUND a constant.

`STORAGE_SIZE(A [, KIND])` is the size in bits of an element of A, a constant
for variables, arrays and array elements of intrinsic or derived type. A derived
type is laid out as gfortran lays it out: each component at the next multiple of
its alignment (its size, or 1 for CHARACTER), and the whole padded to the largest
alignment, so `type :: t; character(3) :: c; integer :: i; end type` has 64 bits.
Types with ALLOCATABLE or POINTER components are not yet supported.

Assigning an INTEGER value of a wider kind to a narrower variable keeps the
value's low-order bits, as gfortran does: assigning 130 to an INTEGER(1)
variable stores -126.

`ISHFTC(I, SHIFT[, SIZE])` rotates the rightmost SIZE bits of I (all of its bits
by default) and is lowered to the operations above. It is not yet accepted in
PARAMETER initializers. `LGE`, `LGT`, `LLE` and `LLT` compare CHARACTER values
in ASCII order, the same as the relational operators for rfort's ASCII storage.
`BGE`, `BGT`, `BLE` and `BLT` compare INTEGER values as unsigned bit patterns.
A user procedure with one of these names keeps its own definition. Run
`python tests/test_intrinsic_rewrites.py`.

Run `python tests/test_bit_intrinsics.py` for interpreter/native O0/O2 checks
with gfortran comparisons. `tests/125_bit_intrinsics.f90` is the local fixture.

### Rounding, ISNAN, MIN and MAX

`NINT(A[,KIND])` rounds REAL and DOUBLE PRECISION values to the nearest integer,
with halfway cases rounded away from zero. Scalar and supported array arguments,
keyword arguments, and INTEGER result kinds 1/2/4/8 are supported. Inlined
functions and subroutines also preserve early RETURN from explicit-step DO loops,
including nested loops and CYCLE paths. Run `python tests/test_util_correctness.py`
for interpreter/native O0/O2 comparisons with gfortran.

The GNU `ISNAN(X)` extension is supported for REAL kinds 4 and 8. It is pure
and elemental, returning default LOGICAL with the input's shape. NaNs return
true; finite values, signed zero and infinities return false. Scalar expressions
are evaluated once. Arrays, sections, empty arrays and runtime PACK results are
supported, as is the `X=` keyword. Run `python tests/test_isnan.py` for
interpreter/native O0/O2 comparisons with gfortran in GNU mode, including ranks
through 15 and runtime-sized multidimensional arrays.

FLOOR and CEILING accept scalar or elemental REAL inputs, including array
sections and runtime-sized arrays. Both support `A=` and optional constant
`KIND=` (INTEGER kinds 1, 2, 4 and 8), and constant PARAMETER initializers.
Unrepresentable results, including NaN and infinity, report an error instead
of converting silently. Run `python tests/test_rounding.py` for interpreter,
native O0/O2 and gfortran comparisons.

Numeric `MIN` and `MAX` accept two or more same-type, same-kind arguments in
array expressions. Scalar arguments broadcast across conformable arrays;
sections and runtime-length PACK results are supported, including empty arrays.
This also permits scalar extrema of reductions inside array expressions, such
as `x / max(sum(abs(x)), tiny_value)`. Shape mismatches are checked before
runtime-length element loops. Run `python tests/test_numeric_minmax.py` for
interpreter/native O0/O2 and gfortran comparisons. Errors from numeric array
lowering now include the source statement's location.

### Elemental math functions

`SQRT`, `EXP`, `LOG`, `LOG10`, the trigonometric and hyperbolic functions and
their inverses (`ASINH`, `ACOSH`, `ATANH`), `ERF`, `ERFC`, `GAMMA`,
`LOG_GAMMA`, `HYPOT`, `AINT`, `ANINT` and the Bessel functions `BESSEL_J0`,
`BESSEL_J1`, `BESSEL_JN(N, X)`, `BESSEL_Y0`, `BESSEL_Y1` and `BESSEL_YN(N, X)`
call the C math library, in DOUBLE PRECISION. The transformational
`BESSEL_JN(N1, N2, X)` form is not supported. `SIGN` takes
the sign bit of B, so `SIGN(1.0, -0.0)` is -1.0 as in gfortran; `DIM` and
`DPROD` are also supported. They are
elemental, including in PRINT and WRITE lists, and keep the kind of their
argument.

The Fortran 2023 degree functions (`SIND`, `COSD`, `TAND`, `ASIND`, `ACOSD`,
`ATAND`, `ATAN2D`) and half-revolution functions (`SINPI`, `COSPI`, `TANPI`,
`ASINPI`, `ACOSPI`, `ATANPI`, `ATAN2PI`) scale the argument or result of the
ordinary function by pi in the argument's kind. gfortran reduces the angle in
degrees first, so DOUBLE PRECISION results can differ from gfortran's in the
last bit; for example `COSD(90d0)` is about 6e-17, not 0. `ATAN(Y, X)` is
`ATAN2(Y, X)`. Run `python tests/test_intrinsic_rewrites.py`.

Specific names of generic intrinsics are accepted and call the generic: `IABS`,
`DABS`, `FLOAT`, `SNGL`, `DFLOAT`, `IFIX`, `IDINT`, `IDNINT`, `DINT`, `DNINT`,
`DSQRT`, `DEXP`, `ALOG`, `DLOG`, `ALOG10`, `DLOG10`, `DSIN`, `DCOS`, `DTAN`,
`DASIN`, `DACOS`, `DATAN`, `DATAN2`, `DSINH`, `DCOSH`, `DTANH`, `AMOD`, `DMOD`,
`ISIGN`, `DSIGN`, `IDIM`, `DDIM`, `MAX0`, `AMAX1`, `DMAX1`, `MIN0`, `AMIN1` and
`DMIN1`. Run `python tests/test_more_intrinsics.py`.

## Control flow

### DO loops

Counted loops accept `do i=first,last,step` with a nonzero INTEGER step,
including negative steps, named parameters and steps known only at run time,
such as `do i = 1, n, incx`. The control variable must be a declared scalar
INTEGER. Supported scalar INTEGER bounds and the step are evaluated once before
the control variable is assigned. CYCLE advances the index, EXIT preserves it,
and empty loops retain the initial value. A runtime step runs
`max((last - first + step) / step, 0)` iterations and stops the program if it
is zero. Increment overflow of a constant step in the compiler's 64-bit INTEGER
storage reports a runtime error.

Unbounded `do` / `end do` loops repeat until EXIT, RETURN, or STOP transfers
control. CYCLE starts the next iteration; nested loops target the innermost
loop. A bare DO may end with a newline, a comment, or a semicolon. Continued
counted DO and DO WHILE headers retain their control clauses.

The older form `do 10 i = 1, n` (also `do 10, i = 1, n`) ends after the
statement with label 10, which may be CONTINUE or another statement; nested
loops may share it. DO CONCURRENT accepts several index controls with strides
and a final mask, as in `do concurrent (i = 1:n, j = 1:m:2, a(i) > 0)`, and runs
its iterations in order as nested DO loops. Fortran 2023
[REDUCE locality](fortran2023.md#do-concurrent-reductions-us-20) supports scalar
and fixed-size explicit-shape intrinsic arrays. A type specification and other locality
specifiers such as LOCAL are not supported.

Counted, unbounded, and DO WHILE loops accept construct names, for example
`do_i: do i=1,ni` with `end do do_i` (or `enddo do_i`). `CYCLE do_i` and
`EXIT do_i` target that enclosing loop even from nested loops, BLOCK, IF, or
SELECT CASE constructs. Names are case-insensitive. Named loops require a
matching name on END DO; duplicate names and transfers to non-enclosing loops
are rejected. Unnamed CYCLE/EXIT continue to target the innermost loop.
Run `python tests/test_named_loops.py` for interpreter/native O0/O2 checks with
gfortran comparisons; `tests/137_named_loops.f90` is in the Cargo fixture suite.

### GOTO

`goto 10` and `go to 10` may jump forward to a label in the same statement
list or an enclosing one, leaving any loops and constructs in between, or
backward to a label earlier in the same statement list, which repeats the
statements from the label. A jump into a construct, such as into a DO body from
outside it, is diagnosed. A region that a GOTO creates may not contain an
unnamed EXIT or CYCLE of an enclosing loop; name the loop instead. A computed
GOTO, `go to (10, 20, 30) k`, jumps to the k-th label, or continues with the
next statement if k is out of range. Jumps that nested loops cannot express,
such as a computed GOTO into the middle of a statement range that a later
GOTO repeats, are compiled as a loop over the labelled segments of the
statement list. Assigned GOTO and the arithmetic IF are not supported.

### Assumed-rank arguments and SELECT RANK

Fortran 2018 assumed-rank intrinsic and nonpolymorphic derived-type dummies
can be declared with `x(..)` or `dimension(..)`. A specific internal or module
procedure can receive a scalar or an array through rank 15. `SELECT RANK` selects a rank-specific block:

```fortran
subroutine describe(x)
real, intent(in) :: x(..)
select rank(x)
rank(0)
   print *, "scalar", x
rank(1)
   print *, "vector", size(x), sum(x)
rank(2)
   print *, "matrix", shape(x), sum(x)
rank default
   print *, "other rank", rank(x)
end select
end subroutine
```

Numeric, LOGICAL and supported CHARACTER actuals work, including runtime-sized
arrays and supported sections. Nonpolymorphic derived arguments use the same
rank specialization: scalar/fixed-size components, runtime-sized arrays,
strided sections, and supported allocatable components retain the existing
derived-type storage behavior. Constructor actuals and separately compiled
module types are supported. `select rank(a => x)` gives a branch-local
associate name; writable branches retain the dummy's INTENT and update the
actual. Multiple assumed-rank dummies, forwarding to another specific
assumed-rank procedure, keyword association, and guarded OPTIONAL arguments are
supported. Functions also use the existing scalar-result and array-call
lowering, with its existing restrictions. Named constructs and integer named
constant guards are accepted. `RANK(A)` and `RANK(A=...)` work for ordinary
scalar/array expressions as well as assumed-rank inputs, without reading the
object's value. `-std=f2008` and earlier reject the new syntax.

The source checker rejects duplicate/out-of-range guards, VALUE and ELEMENTAL
assumed-rank dummies, declarations outside dummy arguments, and direct value
access outside a rank-specific block. A RANK DEFAULT associate remains
assumed-rank: it can be used in supported inquiries or forwarded, but cannot be
printed, subscripted or assigned directly.

Fortran calls specialize a source procedure for the actual ranks before the
usual array/initialization lowering. Combined source files and separately
compiled `.rmod` module interfaces work; this is not a public assumed-rank
separate-object ABI. Assumed-rank BIND(C) descriptors, polymorphic dummies,
generic interfaces containing assumed-rank specifics, and whole assumed-size
actuals/RANK(*) dispatch remain unsupported. RANK(*) guards can be parsed and checked, but the corresponding assumed-size calls are unsupported.
Other inquiry/storage forms retain the restrictions of the existing array
backend. Run `python tests/test_assumed_rank.py` and
`python tests/test_assumed_rank_derived.py`. Standalone examples are
`tests/programs/xassumed_rank.f90` and
`tests/programs/xassumed_rank_derived.f90`.

### SELECT CASE ranges

Integer `SELECT CASE` accepts comma-separated values and inclusive ranges using
`:` (for example, `case (1,3:5,9:)` or `case (:0)`). Bounds must be constant
integer expressions; duplicate/overlapping ranges are diagnosed. Literal and
arithmetic selectors work in native builds, including selectors made constant
by optimization. CHARACTER ranges remain unsupported. Run
`python tests/test_case_ranges.py` for interpreter/native O0/O2 and gfortran checks.
Two-element lists such as `case (EULER, HEUN)` accept named constants and signed
integer values, including unary `+`. Lists also work in named constructs and
continued statements. Complex literals retain their expression syntax in case
bodies and procedure arguments. Run `python tests/test_select_case_lists.py`;
`tests/programs/xselect_case_list.f90` is a standalone example.

CHARACTER selectors may be fixed or deferred-length variables, literals,
substrings, concatenations, and supported CHARACTER-valued expressions. The
selector is evaluated once; comparisons use its active length and Fortran blank
padding, including empty strings. Run
`python tests/test_select_character_expressions.py` for native O0/O2,
interpreter, and gfortran comparisons.

Supported array-input functions, array-valued functions and array-argument
subroutines are specialized inside every CASE and CASE DEFAULT branch, including
nested SELECT CASE constructs. Calls remain in their selected branch and use
the surrounding scope's array descriptors. Run
`python tests/test_select_case_calls.py` for interpreter/native O0/O2 checks
with gfortran comparisons, including the reduced `xacv.f90` median call.

Function-result assignments inside CASE and CASE DEFAULT branches are included
in semantic checks, including nested blocks and selections. Assignment type
checking also visits these branches. See
`tests/programs/xselect_case_function_result.f90` and
`tests/test_select_case_function_result.py`.

### ASSOCIATE

ASSOCIATE replaces each associate name by its selector inside the block, so a
variable, element, section or component selector is updated through the name.
A selector whose value could change inside the block, such as `a(i)` when the
block assigns `i`, is rejected because it would be re-evaluated. ASSOCIATED
also accepts a TARGET (a TARGET variable, section or POINTER) and is true when
the pointer is associated with the same elements in the same order. Run
`python tests/test_associate.py`.

### ERROR STOP

ERROR STOP is permitted in PURE and implicitly pure ELEMENTAL procedures,
as in Fortran 2018. Its code expression is checked for impure calls, and
unconditional error termination ends definite-assignment analysis for that path.
Plain STOP remains forbidden in pure procedures. ERROR STOP accepts scalar
CHARACTER expressions, including runtime variables, concatenations, substrings,
array elements and supported function results, as well as default INTEGER constants.
The message is evaluated when the statement executes; CHARACTER error termination
returns status 1. Runtime INTEGER codes and QUIET are not yet supported.
`tests/programs/xerror_stop_character.f90` and `python tests/test_runtime_error_stop.py`
cover runtime messages in interpreted and native execution.
`tests/programs/xpure_error_stop.f90`,
`tests/173_pure_error_stop.f90`, and `python tests/test_pure_error_stop.py`
cover successful guards, error termination and integer exit statuses.

## CHARACTER

### CHARACTER declarations and lengths

Scalar CHARACTER declarations accept constant integer length expressions,
including named parameters, LEN/LEN_TRIM of character constants, and
constant MIN/MAX and ABS calls.
`character(n)` and `character(len=n)` are supported; negative lengths become
zero. LEN of a fixed-length scalar variable does not require it to be initialized.
Assignments pad with spaces or truncate to the declared length, including
variable copies, concatenation, TRIM, and overlapping source/destination storage.
This subset uses non-NUL ASCII. Fixed rank-one CHARACTER arrays are supported
as described below. Scalar and elemental CHARACTER functions use the
specialization described under [Scalar CHARACTER functions](#scalar-character-functions)
and [ELEMENTAL functions](#elemental-functions). Runtime-dependent explicit lengths and
CHARACTER functions outside those subsets remain unsupported and produce
diagnostics. Bounded deferred-length scalar assignment is described below.

Large fixed CHARACTER storage in the main program, including generated
concatenation, REPEAT, adjustment, and output buffers, uses static storage
rather than enlarging the native stack frame. Native procedure buffers remain
per invocation. `python tests/test_large_character_storage.py` checks large
strings, multiple buffers, and recursive local storage.

CHARACTER declarations accept per-entity lengths, for example
`character(2) :: a*3, b, c*(n+1)`. An entity length overrides the shared length
without changing its neighbors. Scalar PARAMETER entities also accept `*(*)`.
Fixed-length local and host CHARACTER variables may supply `LEN(variable)` in
later CHARACTER length declarations, without reading their values. Runtime
lengths remain unsupported. Run
`python tests/test_entity_character_lengths.py` for interpreter/native O0/O2
and gfortran checks.

Constant `TRIM` and `LEN_TRIM` calls, including CHARACTER PARAMETER arguments,
are evaluated before code generation.

### CHARACTER named-constant substrings and string intrinsics

Scalar CHARACTER PARAMETER values support substring ranges such as
`upper_case(n:n)`. Constant bounds fold to CHARACTER literals, including in
PARAMETER initializers and LEN inquiries. Runtime bounds use the same checks
as scalar variable substrings. Omitted bounds, padded declared lengths and
empty ranges are supported; scalar substring strides and writes to PARAMETER
values are rejected. CHARACTER array PARAMETER sections remain array sections.

`INDEX`, `SCAN` and `VERIFY` support scalar CHARACTER arguments, optional BACK
and constant KIND (1, 2, 4 or 8), positional and named arguments, PARAMETER
initializers and PURE procedures. They search the full string, including trailing
blanks. INDEX returns one for an empty substring, or LEN(STRING)+1 with BACK true.
Rank-one CHARACTER array arguments are expanded elementally with shape checks;
BACK currently must be scalar. LEN_TRIM and character-code conversions also work
on fixed rank-one CHARACTER arrays. Run `python tests/test_character_intrinsics.py`
and `python tests/test_parameter_substrings.py` for interpreter/native O0/O2
checks and gfortran comparisons. `tests/157_parameter_substrings.f90` reproduces
the `upper_case_str` blocker in `util.f90`.

ADJUSTL/ADJUSTR work in assignments, concatenations, comparisons and fixed
rank-one array expressions; results keep the input length. Old-style
`character*5` and `character*(expression)` declarations are accepted. NEW_LINE
supports scalar CHARACTER arguments. Formatted PRINT may omit its output list,
and the colon format descriptor ends processing when the output list is exhausted.
Intrinsic operators may be extended for unsupported operand combinations such as
CHARACTER + CHARACTER; intrinsic arithmetic cannot be replaced. Run
`python tests/test_character_operators.py` for the CHARACTER operator subset.

### Scalar substrings

Scalar CHARACTER variables support substring reads with constant or runtime
scalar INTEGER bounds, including omitted endpoints and empty ranges. String
literals also accept substring syntax, such as `'0123456789'(first:last)`.
Runtime bounds are evaluated once and checked against the current string length
before reading its storage. Substring assignments still require constant bounds.
Overlapping
assignments capture the right-hand side before updating the destination;
shorter values are blank-padded and longer ones truncated. LEN of a substring
uses its declared bounds without reading the parent string. A string may be
defined one substring at a time, as `job(1:1) = 'S'` then `job(2:2) = 'V'`.
Initialization tracking is not yet per character, so after a partial write the
whole string counts as defined. Dynamic assignment bounds, substrings of
PARAMETER strings, READ destinations and substring actuals to subroutines remain
unsupported. Run `python tests/test_scalar_substrings.py`.

### Deferred-length scalar CHARACTER

Main-program and BLOCK locals declared `character(len=:), allocatable` support
assignment from literals, supported scalar CHARACTER expressions and substrings.
Assignment adjusts the logical length, including shrinking to zero, while
preserving overlapping source values. LEN, LEN_TRIM, ALLOCATED, concatenation,
comparisons and supported output operations use the current value and length.
For example:

```fortran
implicit none
character(len=:), allocatable :: s
integer :: first=3, last=7
s = '0123456789'(first:last)
print '(a)', s  ! 23456
end
```

When assignment sources have a known maximum length, this subset can reserve
that bounded storage. Otherwise main-program and BLOCK-local deferred-length
scalars use owned heap storage with a runtime length, including
`s = repeat('ab', n)`, repeated growth such as `s = s // 'x'`, and
`allocate(character(len=n) :: s)`. Assignment can grow or shrink the allocation,
including to length zero, and overlapping assignments capture the source before
replacing the destination. DEALLOCATE releases heap storage. The former
4096-character scalar limit no longer applies to these locals. The current
byte-offset representation limits an individual value/expression to
2,147,483,646 characters; allocation can fail earlier if memory is unavailable.
ALLOCATE with a type specification sets the length; the value is undefined until
assigned. A deferred-length variable can be the VALUE of GET_COMMAND_ARGUMENT
and GET_ENVIRONMENT_VARIABLE, which fill its current length.

Deferred-length CHARACTER arrays (`character(len=:), allocatable :: w(:)`)
are supported as main-program, BLOCK and procedure local variables. ALLOCATE
with a type specification sets the length, which may be a runtime value, as in
`allocate(character(len=n) :: w(m))`. Assignment of a whole array value
reallocates the array to the value's length and size: a constructor, typed or
not, another CHARACTER array, or a value that refers to the array itself, as in
`w = [w(2), w(1)]` or the growth idiom `w = [w, item]`. Element assignment pads
or truncates to the current length, and LEN, SIZE, ALLOCATED, DEALLOCATE,
substrings of elements and internal READ and WRITE with an element work. When
every assignment and ALLOCATE gives the same constant length, the array is
stored with that fixed length. Otherwise each element has room for at least 4096
characters, and a longer length stops the program. Deferred-length array
dummy arguments, function results and module variables are not yet supported,
nor are constructors with array sections or implied DO loops whose length is
set at run time. Programs using runtime-length arrays are slow in the
interpreter (`rfort run`); see [interpreter-todo.md](interpreter-todo.md).
Scalar derived objects may also contain deferred-length allocatable CHARACTER
array components. Their element length accompanies the component storage when
the object is passed to a procedure, including writable dummies. Component
assignment and record copying retain the logical length; a length change
reallocates storage after capturing the source. Explicit allocation, empty
arrays, zero-length elements and deallocation are supported. This component
path currently uses a checked 4096-character limit per element. Run
`python tests/test_deferred_character_components.py`; the small standalone
repro is `tests/programs/xdeferred_character_component.f90`.
Some function specializations still require bounded CHARACTER array inputs;
support for a component in a subroutine does not remove that separate limit.
Local deferred arrays also support overlapping concatenation assignments such
as `w = w // '!'`.
`x%len` and `x%kind` are LEN(x) and KIND(x). Procedure and module
storage and host association, READ into a deferred-length destination, and
MAX and MIN of CHARACTER values retain restrictions. REPEAT, concatenation,
ADJUSTL/ADJUSTR and native output temporaries use heap buffers sized to the
runtime value. REPEAT checks negative counts and excessive result lengths before
multiplication/allocation, and an empty source takes constant time even with a
large count. Each native expression site reuses its buffer across loop iterations;
all such buffers are released on normal procedure returns, including early returns.
Recursive invocations own separate buffers. Deferred-length arrays and the
explicit runtime-length function-result path still retain their documented bounds.
Run `python tests/test_runtime_character_storage.py` for long-string, resizing,
substring-overlap and buffer-reuse checks. Other intrinsic/procedure calls with runtime-length CHARACTER
arguments are diagnosed unless implemented for this subset (such as TRIM).
Runtime substring conditions are reevaluated on each loop iteration,
including after CYCLE. The scalar substring tests compare interpreter and native
O0/O2 results with gfortran when available.

### Character codes

Scalar `ICHAR`/`IACHAR` and `CHAR`/`ACHAR` are accepted in PURE procedures.
Character-code conversions work with runtime one-character substrings;
`ICHAR`/`IACHAR` require length one and return a default INTEGER. `CHAR`/`ACHAR`
accept scalar INTEGER arguments and currently support codes 1 through 127,
consistent with the non-NUL ASCII storage subset. Optional KIND arguments are
not implemented. Variable-bound scalar substring assignments snapshot the RHS,
truncate or pad it to the selected width, preserve the remaining characters,
and check bounds at runtime. Empty destination ranges perform no write.
After a runtime partial update the whole destination counts as defined; bounded
deferred-length strings retain their allocation and length after such updates.
`tests/153_to_lower.f90` reproduces `xto_lower.f90`; run
`python tests/test_character_codes.py` for interpreter/native O0/O2 checks and
gfortran comparisons.

### CHARACTER comparisons and function arguments

Scalar CHARACTER comparisons support all six relational operators with blank
padding of the shorter operand. Fixed-length variables, ASCII literals,
concatenations, TRIM and zero-length strings are supported. Comparison captures
in DO WHILE conditions are reevaluated on every iteration, including after CYCLE.
Run `python tests/test_character_comparisons.py`.

Functions using the numeric/LOGICAL result ABI can receive fixed-length,
read-only scalar CHARACTER arguments.
Actuals may be variables, literals or fixed-length concatenations; they must be
at least as long as the dummy, and longer actuals contribute their leading
characters. Function-to-function forwarding is supported. Writable CHARACTER
function dummies, runtime-length actuals and assumed-length dummies remain
unsupported for that ABI; the CHARACTER-result specialization above supports
assumed-length scalar inputs.

### CHARACTER arrays

Fixed-size rank-one CHARACTER arrays of up to 1024 elements use one scalar per
element. Larger ones, and ALLOCATABLE arrays allocated with more than 1024
elements, are allocated on the heap like automatic arrays; `rfort run`
interprets these slowly, at about a millisecond per element operation.
Supported operations include literal constructors, main-program
declaration initialization, scalar fills, whole-array copies, constant element
indices, runtime element indexing, constant sections (including strides), and
elementwise substring assignment.
All source values are captured before overlapping destinations are written.
Array comparisons support scalar broadcasting; their LOGICAL results use the
ordinary reductions, including constant-DIM ALL/ANY/COUNT. List-directed PRINT, whole-array SIZE and LEN are supported. Initialization is
tracked per element, with diagnostics naming the original array and its storage
element number. Partial substring writes still require initialized elements.

Dynamic accesses check bounds and initialize only the selected element; DO WHILE
conditions with dynamic CHARACTER indexing are evaluated on every iteration.
Local CHARACTER array subroutines can specialize fixed/assumed-length dummies
to caller storage, including bounded allocatable outputs.
Higher ranks and the other array
transformational intrinsics remain outside this subset. Local CHARACTER
array PARAMETERs support explicit or inferred rank-one extents. Untyped constructors require equal-length CHARACTER literals
(including substituted constants). Typed `[CHARACTER(LEN=n) :: ...]` constructors
accept constant CHARACTER elements of different lengths and a constant length;
elements are padded or truncated to that length before assignment. Run
`python tests/test_character_arrays.py`.

### CHARACTER array parameters

Local rank-one CHARACTER PARAMETER arrays support explicit extents and `(*)`
extents inferred from their constant constructor. For example,
`character(len=2), parameter :: s(*) = ["aa", "bb", "cc"]` declares three
constant elements. Each element is padded or truncated to the declared length;
`len=*` can infer a common element length from a nonempty constructor.
Constant element references, copies of named array parameters, constructor
implied-DOs, and fixed nonunit lower bounds are supported. Writes and initializer
shape mismatches are rejected. Storage retains the existing local rank-one
CHARACTER array limit of 1024 elements; this does not add general assumed-size
dummy arrays. Run `python tests/test_character_array_parameters.py` for
interpreter/native O0/O2 checks and gfortran comparisons.

## Arrays

### Explicit array bounds

Fixed-shape declarations accept constant INTEGER lower and upper bounds, such
as `integer :: a(0:1,-1:1), v(-2:2)`, including named-parameter expressions.
Element access, sections and section assignments translate to the compiler's
one-based storage. Dynamic supported scalar INTEGER subscripts are checked
against the original bounds before translation; invalid indices cannot wrap
and alias another element. Section endpoints/strides must currently be constant.
Omitted section endpoints use the declared bounds, including for negative
strides; write `v(2:-2:-1)` to reverse `v(-2:2)`.

LBOUND/UBOUND of whole arrays preserve their declared bounds; section bounds
start at 1. Empty dimensions report LBOUND=1 and UBOUND=0. SIZE reports
extents independently of the lower bounds. An assumed-shape dummy `x(:)` retains
its own default lower bound of 1; an explicit-shape dummy uses its declared
bounds and still requires matching extents under the current call specialization.

Runtime-dependent declaration bounds and explicit lower bounds on assumed-shape
or allocatable arrays remain unsupported. Existing limitations on array expressions and procedure arguments
still apply.

### Array bounds inquiries

`LBOUND` and `UBOUND` support statically shaped arrays and sections. A constant
`DIM` selects a scalar bound; omitting it returns a rank-one vector. Positional
arguments and `ARRAY=`, `DIM=`, and `KIND=4` keywords are accepted, as are these
keywords for `SIZE`. Nondefault result kinds and runtime dimensions are not yet
supported. Constant explicit lower bounds are supported as described under
[Explicit array bounds](#explicit-array-bounds);
the existing restrictions on shapes and array storage still apply.
Inquiries use shape metadata and do not require initialized array elements.

### Empty arrays

Fixed-shape numeric and LOGICAL multidimensional arrays may have zero extents.
Whole-array/section operations and reductions with or without DIM handle empty inputs;
constant and runtime subscripts into an empty dimension remain errors. LOGICAL
matrices support element/section assignment, list-directed output and whole-array
COUNT/ANY/ALL. Multidimensional WHERE is also supported for whole-array destinations.
Run `python tests/test_empty_matrices.py` for empty/LOGICAL matrix checks.

`python tests/test_dim_reductions.py` checks multidimensional DIM reductions,
including masks, empty axes, nested reductions, rank-three sections, shape
diagnostics and masked uninitialized elements. Interpreter and native O0/O2
results are compared with gfortran when installed.

### Constructors

Numeric/LOGICAL array constructors can be used in expressions and can contain
fixed-shape array or section items, which are appended in array element order.
Items must have matching element type and kind. Constructor temporaries preserve
overlapping assignment semantics.

Constructor implied-DO expansion can wait until procedure specialization resolves
bounds such as `size(xx)` for an assumed-shape dummy. Runtime-bound constructors
with scalar items also support whole-array assignment to bounded rank-one
numeric/LOGICAL storage. Controls are captured on entry to each implied-DO;
constant positive/negative strides, dependent nested bounds, mixed scalar items,
and empty iteration ranges are supported. Each index uses local storage and
retains its declared INTEGER kind. The enclosing index variable is unchanged.
A temporary captures every result before destination writes; too many elements
or a final shape mismatch produce a runtime error.

Runtime-bound constructors can also be ordinary expression values, including
arithmetic, reductions, procedure arguments, and control expressions. Their
storage grows at runtime, so these uses do not need a compile-time capacity.
`SIZE` bounds use heap arrays' active extents, including multidimensional
`SIZE(array, dim)` and sizes that change after reallocation. This also applies
to constructors passed to `PACK`; run `python tests/test_pack_runtime_constructor.py`
for constructor, mask, empty-result, and reallocation checks.
Nested loops, runtime strides, zero-trip ranges, and whole-array items in array
element order are supported. Bounds and items are evaluated once at their
respective evaluation sites; constructor indices retain local scope. Fixed-length
CHARACTER scalar items also support runtime construction and assignment.
The bounded numeric assignment path remains an optimization for existing cases.
Some array-valued expressions and sections inside runtime constructors, and
CHARACTER function results requiring later specialization, remain unsupported.
PARAMETER and declaration initializers still require constant expressions. CHARACTER scalar-element constructors can be passed to
read-only CHARACTER array dummies, including typed empty constructors. Constant
formats can print fixed whole numeric/LOGICAL arrays in element order; runtime
format strings with whole numeric/LOGICAL array lists remain unsupported.
Run `python tests/test_constructor_expressions.py` and
`python tests/test_runtime_constructors.py` for interpreter/native O0/O2
checks and gfortran comparisons; `tests/147_runtime_constructors.f90` reproduces
`xconstructor_implied_do.f90` and the comparison expression in `util.f90`.

Empty typed bracket constructors preserve the declared element type and kind:
`[real(kind=dp)::]`, `[integer(kind=8)::]`, `[logical::]`,
`[character(len=3)::]`, and `[double precision::]` all have size zero.
Named kind and length parameters, including imported parameters, are resolved
before array lowering. These constructors support type/size inquiries,
reductions, zero-length assignments, numeric and CHARACTER array PARAMETER initializers,
and procedure arguments under the compiler's existing array-shape rules.
Nonempty explicitly typed constructors currently require constant elements.
Untyped empty constructors remain invalid. Run
`python tests/test_empty_constructors.py` for interpreter/native O0/O2 checks
with gfortran comparisons; the Cargo suite includes
`tests/142_empty_constructors.f90`.

### RESHAPE, MOD and MODULO

`RESHAPE(SOURCE, SHAPE, PAD, ORDER)` accepts numeric/LOGICAL array expressions
and a SHAPE constructor whose extents may be evaluated at runtime, including
zero extents. Runtime-sized SOURCE arrays and allocatable function results are
supported. Optional PAD supplies
repeated elements when SOURCE is too short and must have the same type and kind.
Optional ORDER is a constant permutation of the result dimensions specifying
which dimension varies fastest during filling. Keywords may be reordered; extra
SOURCE elements are ignored. Runtime extents must be nonnegative; a short SOURCE
requires nonempty PAD. SHAPE supplied as a runtime array variable and runtime
ORDER vectors remain unsupported. `tests/programs/xreshape_runtime_shape.f90`
reproduces concatenated array inputs reshaped into an allocatable function result.
Run `python tests/test_runtime_reshape.py` for runtime extents and ordering checks.
Run `python tests/test_reshape_options.py` for padding and fill-order checks.
`MOD` and `MODULO` accept
conformable numeric array arguments and scalar expansion, preserving element
kind. Run `python tests/test_reshape_constructors.py` for interpreter/native
O0/O2 checks and gfortran comparison.

### Vector subscripts

Arrays of ranks 1 through 15 support INTEGER vector subscripts. Implemented element
types are INTEGER, REAL, DOUBLE PRECISION, LOGICAL, CHARACTER, and derived types
with supported scalar intrinsic components. Scalar subscripts, triplets and vector
subscripts can be mixed on different dimensions; the first retained dimension
varies fastest. Selectors may be INTEGER arrays,
constructors, constant implied-DOs, sections, supported arithmetic, ABS/MOD/MODULO
or default INT conversions. Reads preserve order and repetitions. Assignments
capture all indices and right-hand values before writing, including scalar fills,
overlapping selections and an array used as its own index vector. Nondefault
lower bounds and INTEGER kinds 1/2/4/8 are supported.

Numeric and LOGICAL selections use active runtime descriptors for allocated source
arrays and index vectors, including empty vectors, without the former 1024-element
selection limit. Numeric POINTER sources and POINTER index vectors use their
active bounds and strides. CHARACTER selections use bounded storage when a small
capacity is available, or heap storage for runtime descriptor-backed sources.
Vector actual arguments are materialized for known INTENT(IN) subroutine dummies;
association with writable dummies is rejected.

Runtime-sized INTEGER selectors passed through assumed-shape dummy arguments
retain their active length, including results of PACK. These selectors can be
used for reads and assignments, including Cartesian selections on multiple axes
and selection of allocatable components in a derived-type constructor.
`tests/programs/xpacked_vector_subscript.f90` is a reduced DataFrame example.
Run `python tests/test_packed_vector_subscripts.py` for active lengths, empty and
repeated indices, overlap, bounds checks, and intrinsic/derived element types.
Known gaps include runtime-sized selectors on explicit-lower-bound source arrays
and automatic COMPLEX result arrays in this procedure-specialization path.

Constant invalid indices and duplicate assignment indices are compile errors;
dynamic indices are checked against the original bounds before any writes, and
dynamic duplicate destinations cause a runtime error. Duplicate indices remain
valid for reads. SIZE, SHAPE, LBOUND, UBOUND, KIND and CHARACTER LEN inquiries use shape
and type information without reading the array or index values. Only selected
source elements must be initialized. Loop-condition captures are reevaluated on
every iteration, including after CYCLE. Run `python tests/test_vector_subscripts.py`
and `python tests/test_vector_ranks.py` for interpreter/native O0/O2 and gfortran checks.

Vector READ destinations and vector selections in WHERE assignments remain
unsupported. Deferred-length CHARACTER array elements remain unsupported. Derived selection
output currently requires fixed result extents. Arrays of derived
types with nested, array, ALLOCATABLE or POINTER components are not yet implemented. Other vector expressions are rejected when they
reach scalar array-index lowering, instead of silently using their first element.

### Runtime sections

Runtime-sized multidimensional sections support scalar and triplet subscripts,
runtime bounds and strides, omitted bounds, reverse strides and empty sections.
They work for local allocatables, automatic locals, module variables, numeric
pointers and assumed-shape dummies, with ranks through 15. Supported element types include
INTEGER, REAL, DOUBLE PRECISION, LOGICAL, COMPLEX, fixed-length CHARACTER and
supported derived types (including scalar CHARACTER components). Assignment
captures overlapping inputs before writing. Section arguments to writable
subroutine dummies use copy-in/copy-out as required by INTENT; RANDOM_NUMBER
sections do not read unset destination elements. CHARACTER section comparisons
and concatenation are lowered element by element. Bounds, shape conformance and
initialization checks remain active. Existing output restrictions for runtime
COMPLEX and whole derived-type arrays still apply. READ into runtime sections
is still outside this subset. Run `python tests/test_runtime_sections.py` for interpreter/native
O0/O2 checks against gfortran; `tests/187_runtime_sections.f90` covers the basic
regression.

The runtime section path also applies to fixed arrays: scalar selectors such as
`a(:,column)` and rank-one triplets such as `a(first:last:step)` can vary at
runtime. Specialized functions preserve these selections in nested array-valued
calls. FORALL captures its iterations before writes, and known function-result
extents are folded before native storage and output are generated. Run
`python tests/test_fixed_runtime_sections.py` for fixed-storage, overlap,
copy-in/copy-out, element-type and rank-15 regressions against gfortran.

### WHERE and LOGICAL arrays

Rank-one LOGICAL arrays support constructors, whole-array and section
assignment, element access, list-directed output, and ALL/ANY reductions.
WHERE accepts single-line assignments and block constructs with plain or masked
ELSEWHERE (including `else where` and `end where` spellings). Masks are captured
before the branch executes; each assignment captures selected RHS values before
writing its destination. Inactive elemental RHS expressions are not evaluated.
Non-elemental reductions still evaluate their complete array argument.

This WHERE subset supports nested constructs and fixed or runtime-sized
multidimensional arrays, with whole-array numeric or LOGICAL destinations.
Runtime masks and assignment snapshots use active descriptor extents and retain
nondefault lower bounds; mismatched shapes are checked before assignment.
Allocatable destinations retain their allocation and shape. Inner masks evaluate
elemental expressions only where the enclosing mask is active; ELSEWHERE never
reactivates elements excluded by that enclosing mask. Section destinations and
CHARACTER destinations remain unsupported. Run `python tests/test_nested_where.py`
for nested mask, shape, and uninitialized-element checks. Run
`python tests/test_runtime_where.py` for runtime arrays and type-bound NaN filling.

### FORALL

FORALL assignments can call supported PURE scalar and array-valued functions,
including numeric array functions and CHARACTER elemental functions. Constructors
can combine scalar items and array-valued results, as in
`[0.0_real64, cumulative_sum(x(:,j))]`. Function arguments can use fixed matrix
sections and empty arrays. Impure calls remain errors.

Every right-hand side is captured before any destination write, preserving
overlapping assignments such as swapping matrix columns. Captures run after
procedure argument specialization when assumed-shape dummy extents were unknown
in the original template. Runtime scalar INTEGER bounds and strides, including
negative strides, are supported for fixed destinations and descriptor-backed
numeric or LOGICAL destinations. Scalar SIZE, LBOUND and UBOUND inquiries on
descriptor-backed arrays can supply bounds and strides, including inside
array-valued functions. Their allocation checks remain active. Controls are evaluated
once, and duplicate destination elements cause a runtime error. The enclosing
index variables keep their original values. Constant triplets retain their
existing compile-time expansion and destination checks. Existing limits remain:
one assignment per FORALL and bounded array-result specialization. Runtime-sized
destinations require descriptors; runtime-sized CHARACTER destinations remain
unsupported. Calls in WHERE remain
outside this subset. Numeric, LOGICAL and fixed rank-one CHARACTER destinations
are supported. Run `python tests/test_forall_functions.py` and
`python tests/test_runtime_forall.py`.

CHARACTER array arguments retain their active lengths when their backing storage
has greater capacity. Function and subroutine specialization revisits nested
calls exposed by descriptor association, including scalar search functions inside
array-valued search functions. This supports the `unique_characters` utility
driver's unique values, first positions, frequencies and group numbers. Run
`python tests/test_character_active_extent.py` for nested calls, differing
CHARACTER lengths, empty arrays and repeated allocation regressions.

### Reductions

Numeric reductions (`SUM`, `PRODUCT`, `MINVAL`, `MAXVAL`) accept `ARRAY=`,
`DIM=`, and `MASK=` keywords, including reordered keywords. Masks may be scalar
LOGICAL values or conformable LOGICAL arrays/expressions. A constant or runtime `DIM` reduces
one axis of a fixed-shape or runtime-sized array, preserving the other axes in
order; without DIM the whole array is reduced. ALLOCATABLE arrays and supported
assumed-shape dummies/sections use their active extents, through rank 15. Runtime
DIM is evaluated once and checked against the source rank. Earlier procedure-call specialization can still reject
DIM results from runtime-sized arrays passed directly as actual arguments; assign the result to an
ALLOCATABLE array first in those cases.
A LOGICAL array in the second position is the MASK. Empty/all-false integer extrema
use the argument kind's largest/smallest representable values.
Run `python tests/test_masked_reductions.py` for interpreter/native O0/O2 checks
and comparison with gfortran when installed.

`IANY`, `IALL` and `IPARITY` reduce INTEGER arrays with IOR, IAND and IEOR, and
`PARITY` reduces a LOGICAL array with .NEQV.; they take DIM and MASK as the
numeric reductions do. `NORM2(X [, DIM])` is `SQRT(SUM(X*X [, DIM]))`, without
the scaling that avoids overflow for very large elements. The shared runtime
DIM path also supports these reductions and NORM2. Run
`python tests/test_more_intrinsics.py`, `python tests/test_runtime_dim_reductions.py`,
and `python tests/test_variable_dim_reductions.py`.
The latter compares interpreter/native O0/O2 results with gfortran and covers
all axes, kinds, masks, empty slices, nested reductions, result assignment,
sections, non-default bounds, ranks through 15, and allocation/shape errors.

Known ELEMENTAL functions also retain their result types when runtime array
expressions are lowered inside reductions. This includes overloaded derived-type
comparisons such as `all(dates(2:) < dates(:n-1))`, nested numeric elemental calls,
and numeric/LOGICAL elemental expressions used as masks or location inputs.
Run `python tests/test_runtime_elemental_reductions.py` for interpreter/native
O0/O2 checks, including pure/impure procedures and empty arrays.

Logical reductions `COUNT`, `ANY`, and `ALL` accept `MASK=` and `DIM=` keywords.
`COUNT` supports constant `KIND=4` (default) and `KIND=8`; `ANY` and `ALL` return
LOGICAL. Whole-array and constant or runtime DIM reductions support logical arrays,
sections, and relational array expressions. Empty inputs yield 0, false, and
true respectively. Run `python tests/test_logical_reductions.py` for interpreter,
native O0/O2, and gfortran comparison checks.

### DOT_PRODUCT

`DOT_PRODUCT(VECTOR_A, VECTOR_B)` supports rank-one arrays of the implemented
INTEGER kinds 1/2/4/8, REAL and COMPLEX kinds 4/8, including mixed numeric
types/kinds, and pairs
of LOGICAL arrays. Numeric results use multiplication followed by summation;
complex results conjugate the first operand. LOGICAL results use `ANY(a .AND. b)`.
Empty vectors return numeric zero or logical false. Keyword arguments, array
expressions, supported sections/vector selections, procedure arguments and PURE
procedures use the same lowering. Result-kind inquiries do not read array values.

The compiler rejects invalid ranks/types and statically unequal lengths before
flattening arrays. Runtime-sized arrays use the existing conformance checks.
Array-expression and storage restrictions of the reduction machinery still
apply. Run `python tests/test_dot_product.py`; the numbered suite includes
`tests/186_dot_product.f90`.

### Array transformations and locations

CSHIFT supports fixed-shape and runtime-sized numeric, LOGICAL and CHARACTER arrays with
constant DIM. INTEGER SHIFT may be scalar or an array shaped like ARRAY with DIM removed, with runtime
values in either case. Sections, array expressions, empty arrays and overlapping
assignments are supported. Runtime DIM remains unsupported.
Run `python tests/test_cshift.py` for interpreter/native O0/O2 checks and a
gfortran comparison.

TRANSPOSE accepts fixed-shape and runtime-sized numeric or LOGICAL rank-two
arrays, including supported sections and expressions. Runtime results are
materialized with reversed extents before assignment or procedure argument
association; overlapping assignment, nondefault bounds and empty arrays are
supported. The MATRIX keyword and PURE usage are supported.
Run `python tests/test_runtime_transpose.py` for runtime coverage.
Run `python tests/test_matrix_transforms.py` for TRANSPOSE and array-valued
CSHIFT checks in the interpreter and native O0/O2 builds, plus gfortran comparison.

EOSHIFT supports fixed-shape and runtime-sized numeric, LOGICAL and CHARACTER arrays with
constant DIM.
SHIFT and optional BOUNDARY may be scalar or arrays shaped like ARRAY with DIM
removed. Vacated elements use BOUNDARY, or zero/false/blanks when it is omitted.
Sections, overlapping assignments and PURE usage are supported; runtime DIM
remains unsupported. Runtime CSHIFT/EOSHIFT results support
assignment and direct procedure arguments, including nondefault bounds, supported
sections, empty arrays and overlapping assignment. Array SHIFT and BOUNDARY
arguments are checked against ARRAY's shape with DIM removed. Extreme INTEGER(8)
shifts are handled without adding an out-of-range EOSHIFT offset to an index.
Run `python tests/test_eoshift.py` and `python tests/test_runtime_shifts.py`.

SPREAD supports numeric/LOGICAL scalar, fixed-shape and runtime-sized array
sources with constant DIM and constant or runtime NCOPIES. It inserts the
replication dimension at DIM; zero or negative NCOPIES produces an empty result. Sections, nested calls, constructors, overlapping
assignments and PURE usage are supported. Runtime results can be passed directly
to assumed-shape procedures. Runtime DIM and CHARACTER sources remain unsupported.
Run `python tests/test_spread.py` and `python tests/test_runtime_spread.py`.

PACK supports fixed-shape numeric/LOGICAL arrays with scalar or conformable
LOGICAL masks. A supplied rank-one VECTOR sets the result length and supplies
trailing values after selected ARRAY elements. Without VECTOR, runtime masks are
supported by bounded temporary storage with a separate result length. Such results
work in list-directed printing, basic arithmetic/comparisons, whole-array assignment
to rank-one fixed-size arrays, and reductions without DIM/MASK options. Assignments
and array expressions check shape conformance at runtime. Further transformations,
constructors, general array inquiries and WHERE assignments involving runtime-sized
results remain unsupported. Rank-one runtime PACK results can also be passed to
specialized INTENT(IN) array dummies, including nested PACK arguments; dummy
SIZE/LBOUND/UBOUND use the logical result extent. Run
`python tests/test_runtime_pack.py` and `python tests/test_recursive_allocatables.py`.
Derived-type PACK supports ARRAY ranks through 15, runtime extents, sections,
constructors, scalar or conformable LOGICAL masks, and an optional runtime-sized
rank-one VECTOR of the same type. It preserves column-major element order,
evaluates the mask once, and reads only selected records plus the required VECTOR
tail. Nested records and fixed-size component arrays are supported; records with
owned ALLOCATABLE components are rejected. Negative-stride PACK sections currently
require explicit lower and upper bounds. Assignment to records with CHARACTER
array components currently requires an explicitly allocated destination. User-defined
procedures named PACK retain their usual dispatch. Run `python tests/test_derived_pack.py`.
Allocatable assignment also retains ELEMENTAL result type metadata, and native
LOGICAL function results correctly return literal truth values.

UNPACK supports a rank-one VECTOR, fixed-shape or runtime-sized LOGICAL MASK
and scalar or conformable FIELD of the same type/kind as VECTOR. Runtime VECTOR
and FIELD arrays, supported sections, direct procedure arguments, empty arrays
and overlapping assignment are supported. Array FIELD shape is checked against
MASK, and VECTOR must have enough elements for the true MASK elements; extra
VECTOR elements are ignored. Elements are consumed in Fortran array-element
order. CHARACTER VECTOR and FIELD must have matching element lengths.
Run `python tests/test_pack_unpack.py` and `python tests/test_runtime_unpack.py`.

CHARACTER CSHIFT, EOSHIFT and UNPACK preserve element lengths and trailing blanks.
EOSHIFT's omitted boundary fills with blanks; supplied CHARACTER boundaries
must match ARRAY's element length. Fixed-length arrays (including runtime-sized
multidimensional arrays) and supported deferred-length rank-one arrays work with
assignment, sections, overlap and procedure arguments. Deferred-length operands
are supported as whole arrays or sections. Runtime length checks use
the actual deferred length, not its storage capacity. Zero-length elements and
empty arrays are supported. The existing deferred CHARACTER storage-capacity
limits still apply. Multidimensional allocatable CHARACTER assignment reallocates
when the shape changes, preserves existing lower bounds when it does not, and
uses a whole-array source's lower bounds when reallocation is required.
Run `python tests/test_character_transforms.py` for coverage; the deferred-length
interpreter case has a longer timeout because its bounded CHARACTER temporaries
are slower than native execution.

MINLOC/MAXLOC support fixed-shape and runtime-sized numeric arrays through
rank 15, including supported ALLOCATABLE arrays, dummies, sections and elemental
expressions; FINDLOC also supports LOGICAL and CHARACTER arrays, including fixed
and runtime extents and allocatable components. Optional constant or runtime DIM reduces rank, or omitting DIM returns one position
per source dimension. MASK may be scalar or conformable LOGICAL; runtime scalar
BACK selects the last matching location instead of the first. Empty selections
return zeros. KIND=4 and KIND=8 are supported. FINDLOC VALUE must currently have
the same type/kind as ARRAY. CHARACTER lengths may differ: comparison pads the
shorter operand with blanks, preserving nonblank suffixes in VALUE. CHARACTER
searches support sections, empty arrays, MASK, BACK, DIM, KIND and keyword arguments.
Use MASK= when omitting DIM; the positional MASK-only form,
deferred CHARACTER lengths and runtime-sized PACK inputs remain outside this
subset. Run `python tests/test_locations.py` and `python tests/test_character_findloc.py`.

## ALLOCATABLE arrays and pointers


`MATMUL` multiplies REAL, DOUBLE PRECISION and INTEGER matrices and vectors of
constant size, including mixed types (the result has the type of the product),
section and TRANSPOSE operands, and checks that the inner extents agree.
ALLOCATABLE, assumed-shape and other runtime-sized operands are also supported,
with any lower bounds: the product is computed into a temporary before the
statement, and operands whose inner extents differ stop the program with
"MATMUL operand shapes do not conform". A runtime-sized operand must be a whole
array, an elemental expression of whole arrays, or a TRANSPOSE or RESHAPE of
one; other expressions, such as sections, must first be assigned to an
ALLOCATABLE array. LOGICAL MATMUL is rejected. Run
`python tests/test_runtime_matmul.py`.
`MERGE` is elemental: TSOURCE and FSOURCE may be numeric, LOGICAL or fixed-length
CHARACTER values; MASK must be LOGICAL. Scalar arguments broadcast over conforming
array arguments. CHARACTER sources must have equal lengths; the destination's
length does not relax that requirement. Fixed-size CHARACTER arrays and sections,
nested calls, keyword arguments and use inside PURE/ELEMENTAL procedures are
supported. Results are materialized before assignment, including when the
destination also appears as a source. A scalar mask expression is evaluated once
when broadcast over an array. Runtime-sized rank-one arrays also support
fixed-length CHARACTER MERGE, including scalar broadcasting, numeric expression
masks, whole arrays and constant-stride sections. Runtime shapes are checked;
allocatable destinations are allocated or resized as needed. A heap temporary
preserves overlapping sources, and this runtime path has no 1024-element limit.
The fixed-extent expansion remains limited to 1024 elements. Runtime-sized
multidimensional arrays and deferred CHARACTER element lengths remain unsupported
for CHARACTER MERGE. Numeric and LOGICAL array MERGE accepts runtime-sized
arguments, as in `merge(0.0, 1.0, x <= 0.0)` for an ASSOCIATE name or section
of an ALLOCATABLE array, and checks that they conform at run time. Run `python tests/test_character_merge.py` and
`python tests/test_formatted_output.py`. The examples
`tests/programs/xmerge_char.f90`, `tests/programs/xmerge_char_vec.f90` and
`tests/programs/xmerge_char_vec_alloc.f90`
exercise scalar and array masks.

`MERGE` also accepts scalar derived-type sources with a scalar LOGICAL mask,
including constructors, function results, array elements, nested calls and
keyword arguments. The sources must have the same derived type. The mask is
evaluated once before assigning the selected whole record, preserving existing
assignment behavior for nested, CHARACTER, fixed-array, ALLOCATABLE and POINTER
components. Scalar results can be passed to typed procedure arguments, included
in array constructors or broadcast to derived arrays. This also works inside
PURE/ELEMENTAL procedures; existing ELEMENTAL call support can apply such a
procedure to record arrays. Direct array-valued derived sources and array masks
for derived `MERGE` remain unsupported. See `tests/test_derived_merge.py`.

### Runtime allocation

Automatic arrays, procedure locals such as `real :: w(n)` or `integer :: a(0:n, m)`
whose bounds depend on dummy arguments, are allocated on entry with the current
bounds and released on return. Each recursive activation has its own array.
Fixed-length CHARACTER and derived-type automatic arrays work the same way.
Like other runtime-sized CHARACTER arrays, an automatic CHARACTER array does not
yet support substring assignment to an element (`c(i)(2:3) = ...`) or internal
WRITE to an element. Automatic CHARACTER lengths such as `character(len=n)` are
not yet supported. Run `python tests/test_automatic_arrays.py`.

Numeric and LOGICAL allocatable locals and module variables now support heap
allocation with runtime dimensions, including dimensions read from input or
computed by a procedure. The shared descriptor tracks allocation status, active
extents and lower bounds; contiguous column-major strides follow those extents.
Ranks 1 through 15 and INTEGER kinds 1/2/4/8 are supported. REAL and DOUBLE
PRECISION arrays also use this path. Fixed-size specializations may still reserve
bounded buffers.

SIZE (including constant DIM), SHAPE, LBOUND, UBOUND, ALLOCATED, scalar filling,
conformable array arithmetic, list-directed output, full-array reductions and
RANDOM_NUMBER use the current allocation. Explicit `lower:upper` allocation
bounds can depend on runtime values. Assumed-shape numeric dummies support
reading and writing these arrays, with dummy lower bounds rebased to one.
Each subscript is checked against its dimension. Allocation-size arithmetic is
checked for overflow, and allocation failure is reported at runtime.

DEALLOCATE releases the data and its initialization mask. Reallocation starts
with unset elements; reads remain checked per element. Module allocations persist
across procedure calls. Heap-backed procedure/BLOCK locals and expression buffers
are released on normal scope exit, including RETURN, EXIT and CYCLE.

### ALLOCATABLE scalars

Intrinsic allocatable scalar locals and module variables support INTEGER,
REAL, LOGICAL, COMPLEX, and fixed-length CHARACTER storage. Explicit allocation,
`ALLOCATED`, deallocation, allocation on intrinsic assignment, `MOVE_ALLOC`,
and the supported `SOURCE=`, `MOLD=`, `STAT=` and `ERRMSG=` options reuse the
existing allocation descriptors. Scalar values retain their original type,
kind and rank during generic resolution and when passed to ordinary scalar
dummy arguments. Supported numeric/LOGICAL pointer association also works with scalar
ALLOCATABLE TARGETs.

```fortran
integer, allocatable :: x
print *, allocated(x)       ! F
allocate(x)
print *, allocated(x)       ! T; x's value is still undefined
x = 42
print *, x
! Releasing storage also releases its initialization state.
deallocate(x)
x = 17                     ! Intrinsic assignment allocates x again.
print *, allocated(x), x    ! T 17
```

Allocation status and value initialization are checked separately. A definite
read before allocation or initialization is rejected at compile time. When
control flow makes the state uncertain, native execution and the interpreter
check it at runtime. Reallocating after DEALLOCATE does not restore an old
value. Local storage is cleaned up on procedure exit; module storage persists.
Type inquiries such as KIND, HUGE and DIGITS do not read the stored value.

SOURCE is evaluated once before any objects in that ALLOCATE statement become
allocated. MOLD supplies type information without reading its value, including
when the mold is an unallocated scalar. SOURCE/MOLD scalar rank, type and kind
are checked when known; fixed CHARACTER lengths must match when determinable.
Fixed-length CHARACTER assignment pads or truncates to the declared length;
substring reads and writes are supported. Deferred-length CHARACTER scalars
continue to use the existing bounded string implementation and its limits.

Intrinsic scalar ALLOCATABLE dummy arguments in subroutines and functions
share allocation status, storage and initialization state with the caller.
An unallocated actual is permitted. INTENT(OUT) deallocates an allocated actual
on entry; INTENT(INOUT), or no INTENT, permits allocation, assignment and
deallocation visible to the caller. INTENT(IN) permits reads and inquiries but
rejects modification. OPTIONAL presence is independent of allocation status.
Numeric, LOGICAL, COMPLEX and fixed/assumed-length CHARACTER scalar dummies
are supported; actual arguments must match type, kind and scalar rank.
Ordinary scalar arguments continue to receive scalar values. Generic dispatch
uses the original scalar rank, before descriptor lowering.

Try `rf.bat xalloc_scalar_dummy.f90 --run`, or run
`python tests/test_allocatable_scalar_dummies.py` after building the compiler.

Intrinsic scalar ALLOCATABLE function results support INTEGER, REAL, LOGICAL,
COMPLEX and constant-length CHARACTER. Functions can allocate explicitly, use
SOURCE/MOLD, or allocate by intrinsic assignment; calls can appear in scalar
expressions, constructors and ordinary input arguments. Each call uses separate
caller-owned storage and initialization state, with automatic cleanup. Repeated
calls, early RETURN, OPTIONAL/keyword arguments, generic resolution, PURE
functions and separately compiled module procedures are supported. DO WHILE
conditions reevaluate result calls at every iteration, including after CYCLE.
Reading an unallocated result reports the function name; reading an allocated
but undefined result retains initialization checks. KIND, fixed-length LEN,
and numeric model inquiries use the result type without evaluating the function.
A function result remains an expression: materializing it does not permit ALLOCATED(f()) or its use as
an ALLOCATABLE or writable variable actual. Deferred-length CHARACTER results
retain the existing length-aware backend and its bounded-string restrictions.
Explicit CHARACTER result lengths can depend on scalar dummy arguments or
supported specification expressions. The length is captured once on entry;
assignment pads or truncates to that declared length, including length zero
when the length expression is negative. LEN queries use a separate length-only
helper so each actual argument is evaluated once. Runtime result values use the
existing 4096-character expression-buffer bound and report an error when it is
exceeded; fixed lengths known at compile time retain statically sized storage.

Direct and mutual recursion support numeric, LOGICAL, COMPLEX, fixed-length
CHARACTER and explicit runtime-length CHARACTER allocatable scalar results.
Each invocation owns its allocation and initialization state. A checked value
buffer transfers the result through the native recursive-array interface before
local storage is released. CHARACTER values use character-code buffers; string
assignment and decoding avoid repeated copies of the full bounded buffer.
Recursive calls retain the existing argument restrictions of that native
interface. Deferred-length CHARACTER results keep their separate backend,
including its recursive-call restriction. Direct recursion requires an explicit
RESULT name, as in `recursive function fact(n) result(y)`.

Run `python tests/test_scalar_result_extensions.py` or try
`rf.bat xalloc_scalar_recursive.f90 --run` for both extensions.

Try `rf.bat xalloc_scalar_result.f90 --run`, or run
`python tests/test_allocatable_scalar_results.py` after building the compiler.

Concrete scalar `TYPE(t), ALLOCATABLE, TARGET` objects also support allocation,
including SOURCE constructors, without requiring a polymorphic pointer owner
in the source group. Saved targets retain their allocation between calls;
see [scalar polymorphic pointer components](#scalar-polymorphic-pointer-components)
for the storage and model-expression restrictions.

Concrete scalar derived allocatables also work without TARGET, including saved
and module storage, with the supported record component layouts. MOVE_ALLOC
transfers their allocation and owned components without copying; scalar concrete
ALLOCATABLE dummy descriptors and allocatable scalar derived components remain
unsupported. Explicit SAVE on intrinsic allocatable scalars and allocatable
scalar DO control variables also remain unsupported. Existing host-association
restrictions for allocatable storage also remain. Ordinary scalar dummy arguments do not gain ALLOCATABLE semantics.

Run `python tests/test_allocatable_scalars.py` after building the compiler, or
try `rf.bat xalloc_scalar.f90 --run` for the allocation-state example above.

### ALLOCATABLE dummy arguments

For scalar polymorphic objects, see [scalar polymorphic allocatable dummy
arguments](#scalar-polymorphic-allocatable-dummy-arguments).

Numeric and LOGICAL ALLOCATABLE subroutine dummies support ranks 1 through 15
with INTENT(IN), INTENT(OUT) and INTENT(INOUT). The dummy aliases the caller's
heap descriptor, including its data, extents, lower bounds, allocation status
and initialization mask. This works for main-program, procedure/BLOCK and module
actuals, including forwarding through another subroutine, optional dummies and
keyword arguments. The actual must be an ALLOCATABLE variable of matching type,
kind and rank. Unallocated actuals are allowed; reading their data or extents
still reports an error.

INTENT(OUT) deallocates an allocated actual on entry, including when the callee
returns without allocating a replacement. ALLOCATE and DEALLOCATE through an
INTENT(INOUT) or INTENT(OUT) dummy update the caller. INTENT(IN) preserves
allocation status and lower bounds and cannot be passed to a writable dummy.
Allocation dimensions can depend on inputs, and outputs can exceed input sizes;
a compile-time capacity bound is no longer needed for numeric dummy outputs.
ALLOCATABLE dummy calls continue to use specialization, with recursive allocation call cycles diagnosed.
A function's ALLOCATABLE INTENT(IN) array dummy that the function does not
inquire with ALLOCATED, LBOUND or UBOUND, or pass on to a subroutine, is treated
as assumed-shape, so the function accepts runtime-sized actuals, including the
ALLOCATABLE components of a derived argument. A procedure with array or derived
dummies can ALLOCATE a local array with an extent known only at run time, such
as `allocate(w(size(x)))`.
Numeric and LOGICAL function dummies also retain their allocation descriptors
with explicit INTENT(IN), INTENT(OUT) or INTENT(INOUT). Unallocated actuals can
be inspected with ALLOCATED; allocated actuals retain their runtime extents and
lower bounds. A writable function dummy can allocate, resize or deallocate the
caller's storage. This also supports scalar ELEMENTAL calls on records with
allocatable components of derived type: their flattened scalar interface carries
the component descriptors, so inquiries do not read uninitialized element data.
Array-valued ELEMENTAL calls with these records retain their existing limits.
Optional, pointer and CHARACTER function dummies retain their existing
specialization restrictions. Run `python tests/test_allocatable_dummies.py` and
`python tests/test_allocatable_function_dummies.py`.

Rank-one CHARACTER ALLOCATABLE dummies support the caller's heap descriptor,
including when an input array or fixed local supplies a known capacity.
An unallocated local output in another procedure needs no prior ALLOCATE.
Assumed-length dummies retain the actual element length, while allocation extents and lower
bounds can be read or computed at runtime. INTENT(OUT) deallocates an allocated
actual on entry; INTENT(INOUT) shares allocation and deallocation with the caller.
Empty results, forwarding and repeated calls are supported. Bounded CHARACTER
function results retain their existing result-storage specialization. Run
`python tests/test_character_allocatable_dummies.py` for interpreter/native
O0/O2 checks against gfortran.

Fixed-length rank-one CHARACTER outputs support ALLOCATED,
SIZE, element access, scalar filling and
conformable whole-array or constant-stride section copying, including padding
and truncation when element lengths differ. Fixed-length multidimensional
CHARACTER arrays also have runtime heap storage,
including section output and copying. General CHARACTER reductions remain
outside this subset.
Growing CHARACTER output cycles without a finite bound remain diagnosed.
Run `python tests/test_recursive_allocatables.py` for recursive OPTIONAL call
coverage and interpreter/native O0/O2 comparisons with gfortran.

### DEALLOCATE

DEALLOCATE accepts lists of whole bounded allocatable arrays and
deferred-length scalar CHARACTER locals, including in single-line IF statements.
It clears ALLOCATED and initialization state, permits subsequent allocation or
CHARACTER assignment, and reports an error when an object is already unallocated.
Numeric locals with runtime allocation dimensions use heap storage; existing
fixed-size and output specializations may retain bounded storage. Different extents
and zero-sized allocations are supported.
Supported allocatable INTENT(INOUT) dummies share allocation state with the caller;
INTENT(IN) objects and nonlocal objects in PURE procedures cannot be deallocated.
Pointers remain outside this DEALLOCATE subset; STAT= and ERRMSG= are described
under [MOVE_ALLOC and ALLOCATE options](#move_alloc-and-allocate-options). Run `python tests/test_deallocate.py` for
interpreter/native O0/O2 checks and comparison with gfortran.

### Reallocation on assignment

Intrinsic assignment of an array value to a whole heap-backed numeric or LOGICAL
ALLOCATABLE array allocates it when unallocated and reallocates it when the
shape differs, as in Fortran 2003. A reallocated target takes the lower bounds
of a whole array source and otherwise starts at 1; a target whose shape already
conforms keeps its bounds. The right-hand side is captured first, so it may
refer to the target. Rank-one targets accept any array expression. Targets of
rank two or more reallocate from whole arrays, sections, and arithmetic and
elemental intrinsic expressions of them, such as `t = 2*c - t` or
`t = sqrt(c) + 1`. Such an expression cannot yet mix fixed-size and
runtime-sized arrays.
Scalar assignment never allocates. Array constructors may contain whole
runtime-sized arrays and sections, so `a = [a, x]` appends. Rank-one
fixed-length CHARACTER arrays reallocate the same way, from arrays, sections and
constructors that mix them with scalars, such as `c = [c, 'new']`. Rank-one
ALLOCATEd locals of procedures with array dummies (which use bounded
call-site storage) are not yet reallocated. Run
`python tests/test_allocatable_reallocation.py`.

### MOVE_ALLOC and ALLOCATE options

`call move_alloc(from, to)` moves an ALLOCATABLE array, including components,
by copying it and deallocating `from`, so pointers into `from` are not carried
over. Concrete scalar derived allocatables instead transfer their field buffers,
owned component handles and initialization masks without copying. Existing
polymorphic pointers follow the allocation when TO has TARGET, including repeated
moves, saved/module storage, and separately compiled module procedures. An
allocated TO is deallocated first; an unallocated FROM leaves both unallocated.
Keyword FROM/TO, STAT and ERRMSG are accepted for these scalar record moves;
success sets STAT to zero and leaves ERRMSG unchanged. See
`tests/programs/xderived_move_alloc.f90` and run
`python tests/test_derived_move_alloc.py`.

ALLOCATE accepts `source=` and `mold=` (taking the bounds of an array
source or mold, or the given bounds), and ALLOCATE and DEALLOCATE accept
`stat=` and `errmsg=`, which report allocating an allocated array or
deallocating an unallocated one instead of stopping. Run
`python tests/test_allocation_options.py`.

### ALLOCATABLE function results

Array transformations, reallocation on assignment beyond the cases described
above, and mixing fixed and runtime-sized multidimensional expressions retain
their existing limitations. Numeric/LOGICAL masked and constant-DIM reductions
use the active extents of supported runtime arrays, as described under reductions.
Deferred-length CHARACTER array elements remain unsupported. No public descriptor ABI is provided
for separate-object module interfaces.
Zero and negative allocation extents produce allocated empty arrays.
Numeric/LOGICAL array-valued functions can use runtime-sized inputs and results
through a hidden allocated result argument, including calls nested in another
procedure's input arguments. ALLOCATABLE numeric/LOGICAL results use caller-owned
descriptors even for fixed-size actual arguments. Intrinsic assignment allocates
or resizes these results to the RHS shape, preserving RHS values before any
reallocation and retaining existing lower bounds when shapes conform. This also
supports filling a result through an ALLOCATABLE INTENT(OUT) helper. Run
`python tests/test_numeric_allocatable_results.py` to check direct assignment,
helper calls, empty/repeated results, element kinds and multidimensional ranks.
Runtime rank-one result section assignments retain
RHS values before writing. CHARACTER functions continue to use bounded storage;
direct constant allocations now supply their active shape before function calls
are materialized. `tests/184_allocated_character_concat.f90` covers the empty
CHARACTER input concatenation and allocation case from `xutil.f90`.

### Pointer association

Scalar and deferred-shape numeric or LOGICAL pointers of ranks 1 through 15
support `p => x` with fixed `TARGET` storage. Deferred-shape arrays also support
allocated `ALLOCATABLE, TARGET` storage of the supported numeric/LOGICAL kinds,
including runtime extents and lower bounds. Association to an unallocated array
is a runtime error. The same descriptor lowering handles
every rank. Descriptors contain the storage and initialization addresses, an
association state, an origin offset, and per-dimension lower bounds, extents,
and strides. The interpreter represents addresses with checked storage handles.

Whole targets retain their lower bounds. Sections support runtime bounds,
strides, and scalar subscripts, captured at association time; scalar subscripts
reduce rank. Noncontiguous and negative-stride views use the original storage.
Pointer-to-pointer association copies the descriptor. Reassociation can change
the shape and total element count, and there is no pointer-specific 4096-element
limit. Whole-array and section assignments snapshot array right-hand sides to
preserve overlapping aliases; snapshots use runtime storage for heap targets.
Matching-shape updates of an allocated target preserve its existing associations.
As in Fortran, a deallocated or reallocated target requires pointer reassociation
before further access. `tests/test_allocatable_pointer_targets.py` checks
whole/section targets, allocation bounds, empty arrays, strides, shared
initialization masks and local procedure dummy arguments against gfortran.

`NULLIFY`, `p => NULL()`, and `ASSOCIATED(p)` are supported; the two-argument
form is described under [ASSOCIATE](#associate).
Array pointers support element and whole-array assignment, section reads/writes,
`SIZE`, `SHAPE`, `LBOUND`, and `UBOUND`, including runtime DIM. Whole-array SUM,
PRODUCT, MINVAL, MAXVAL, ANY, ALL, and COUNT are supported. Reduction DIM
arguments and numeric-reduction MASK arguments remain unsupported for pointers.
Association and shape inquiries do not
read the target's values; ordinary uninitialized-value checks apply when data
is read through the pointer. Unassociated accesses and invalid dynamic
subscripts produce runtime errors. Compiler-managed pointer status starts
disassociated; standard Fortran leaves an uninitialized pointer's status
undefined, so portable code should explicitly nullify or associate it first.

Locally defined subroutines can return associations through scalar or rank-1–15
`POINTER` dummies, including OPTIONAL and named arguments. An `INTENT(OUT)`
pointer loses its association on entry without undefining the target's data.
Targets for escaping associations must be caller-owned `TARGET` variables;
associations to storage in a narrower BLOCK or call scope are rejected.

Numeric and LOGICAL pointer arrays and expressions involving them can be passed
to ordinary read-only `INTENT(IN)` array dummies of supported local procedures,
including scalar-result functions. Owned temporaries preserve active extents,
rank, element order and kinds. This includes empty sections and ranks 1–15;
unassociated accesses and shape mismatches remain runtime errors.
Run `python tests/test_pointer_expression_arguments.py` for regression checks.

The native descriptor is currently lowered to scalar local fields. Procedure
calls still use specialization; a general descriptor calling convention and
pointer ALLOCATE remain future work. Numeric ALLOCATABLE heap storage is
described under [Runtime allocation](#runtime-allocation). Early IR transformations that assume
direct variable access are disabled for procedures that access pointer storage;
Cranelift still applies the requested native optimization level.

Pointer declaration initialization, bounds/rank remapping, pointer
allocation, ALLOCATABLE scalar targets, CHARACTER/derived scalar pointers,
INTEGER(16) pointers, pointer
dummy function arguments and results, host pointer references from unspecialized
procedures, `INTENT(IN)` pointer dummies,
pointer association in PURE procedures, READ through pointers, and passing
pointers to writable ordinary subroutine dummies remain unsupported.

Run `python tests/test_pointer_association.py` for interpreter/native O0/O2
checks with gfortran comparisons. `tests/135_pointer_association.f90` tests the
column-pointer output pattern used by `util.f90`'s `get_data`.
Run `python tests/test_pointer_descriptors.py` for ranks 2–15, changing shapes,
nonunit bounds, strided views, output dummies, overlap, and initialization checks.
`tests/136_pointer_descriptors.f90` covers multidimensional descriptors in the
regular Cargo regression suite.

## Derived types

### Derived-type components

Module-defined types support a `private` statement before `contains` to make
components private by default, and explicit `public`/`private` component
attributes. Module procedures and their internal procedures retain access;
USE association, renaming and re-export do not expose private components.
Nested components, inherited components, array elements/sections, SELECT TYPE
aliases and TYPEOF/CLASSOF declarations retain the access checks. Diagnostics
identify the component and its defining module and include the source location.
Whole-object assignment remains legal; a structure constructor outside the
module may omit default-initialized private components but cannot explicitly
supply them. Whole-object intrinsic I/O outside the defining module cannot
expose private components.

A separate `private` statement after `contains` sets the default accessibility
of type-bound procedures and named generics. Explicit binding attributes override
that default; public generics can call private specific bindings. This default
is independent of component accessibility. See
`tests/programs/xprivate_components.f90` and `tests/test_private_components.py`.

Scalar derived objects can have array, ALLOCATABLE array and POINTER (scalar or
array) components. Each component is stored as its own variable, so element and
section references, ALLOCATE/DEALLOCATE, ALLOCATED/ASSOCIATED, pointer
assignment, NULLIFY and passing a component to an array dummy behave as they do
for ordinary variables. Whole-object assignment deep-copies ALLOCATABLE
components (keeping their bounds, or leaving the target unallocated) and copies
POINTER association. Derived arguments, INTENT(OUT) dummies and derived function
results carry these components. A constructor value for an ALLOCATABLE component
is assigned with the reallocation rules below; `null()` or an omitted ALLOCATABLE
or POINTER component becomes unallocated or disassociated.
A component may itself have a derived type (`type(point) :: a`); `s%a%x`,
whole nested values such as `s%a = point(1, 2)`, nested constructors, a default
such as `type(point) :: hi = point(1, 1)` and passing `s%a` to a `type(point)`
dummy work as for top-level objects. A component may also be a fixed-size or
ALLOCATABLE array of a derived type, as in `type(node), allocatable :: nodes(:)`:
`g%nodes(i)%id`, `g%nodes%w`, sections, ALLOCATE, SIZE, deep copies and passing
`g` to a procedure all work. Its element type may also own supported numeric
or LOGICAL ALLOCATABLE array components, as in a container of independently
allocated vectors. Structure constructors accept derived-type array components
with positional or keyword arguments, including sections, empty arrays and
omitted or NULL() allocatable components. These constructor arguments currently
require element types without their own ALLOCATABLE components.
See `tests/programs/xderived_array_component_constructor.f90`.
POINTER components of derived type and ALLOCATABLE scalar components
(including deferred-length CHARACTER) remain unsupported, and an ALLOCATABLE
component, like any ALLOCATABLE array, must be allocated (by ALLOCATE or by
assignment) somewhere before it is referenced.

Structure constructors accept keyword components, such as `pt(y=2.0)`; omitted
components take their default initialization. A derived object in an output list
stands for its components in order, so `print *, obj` works unless a component is
ALLOCATABLE or POINTER, which Fortran requires defined input/output for. Derived
objects can be named constants (`type(pt), parameter :: origin = pt(0, 0)`), SAVE
locals, initialized variables and module variables, with PUBLIC and PRIVATE; a
named constant or initializer may use another constant of the type. PARAMETER and
saved local objects need scalar components. Run
`python tests/test_derived_array_components.py` and
`python tests/test_derived_type_features.py`.

### Scalar finalization

A derived type defined in a module can declare `final :: release` (or
`final release`). The finalizer must be a module subroutine with one scalar or array,
nonpolymorphic dummy of that type, without POINTER, ALLOCATABLE, OPTIONAL,
VALUE or INTENT(OUT). Invalid bindings receive source locations.

Scalar and array nonpolymorphic objects invoke their finalizers on intrinsic assignment,
explicit deallocation, INTENT(OUT) entry, and automatic procedure/BLOCK cleanup,
including RETURN, EXIT and CYCLE through nested blocks. Assignment evaluates
and preserves the right-hand side before finalizing the destination. Unallocated
objects are not finalized. MOVE_ALLOC finalizes an allocated destination before
replacing its allocation; the source allocation moves without finalization.
The containing object's finalizer precedes finalizers of its nonallocatable
scalar components. Primitive allocatable components remain available during
finalization and are subsequently released by normal allocation cleanup.

Array finalization selects a non-elemental finalizer with the same rank, from
one through fifteen, and passes the whole array once, including zero-sized
arrays. If no matching rank exists, an ELEMENTAL finalizer is applied to each
element. A non-elemental scalar finalizer is not an array fallback. Nested scalar
components are finalized after the containing array's finalizer completes.
Array assignments preserve overlapping RHS values and selected destination
indices before calling FINAL, including strided and vector selections. Whole
ALLOCATABLE arrays can allocate or reallocate on assignment; allocatable array
INTENT(OUT) arguments are finalized before their old allocation is released.
Writable section transfers preserve initialization masks, including numeric and
CHARACTER fields that a finalizer does not read. Transporting an undefined field
does not define it or suppress errors from a subsequent read.

Owned components of finalizable derived type are supported: both scalar
ALLOCATABLE components and array components through rank 15. The containing
object's FINAL procedure runs while its components remain available, followed
by finalization of nonallocatable scalar components, then finalization and
deallocation of allocated components. Unallocated components are skipped.
This also applies to containers without a FINAL procedure of their own,
nested ownership, array owners passed through scalar procedures, intrinsic
assignment after an independent RHS snapshot, INTENT(OUT), MOVE_ALLOC,
explicit deallocation, and procedure/BLOCK exit. Scalar owned components use
one-element internal descriptors with scalar FINAL dispatch. Copies of containers
whose children own intrinsic allocations copy the data independently.

Saved objects and objects remaining at STOP, ERROR STOP or main-program
termination are not finalized for that event. Compiler-generated record copies
are not separately finalized. Private module finalizers remain accessible through
their public type, including USE ONLY, renaming and separate `.rmod` compilation.
These lifecycle rules follow [Fortran 2023 section 7.5.6](https://j3-fortran.org/doc/year/24/24-007.pdf).

The implementation diagnoses polymorphic and POINTER objects, type extension involving finalizable types,
finalizable function results, TYPEOF/CLASSOF of finalizable objects, scalar
ALLOCATABLE dummies, fixed array components of finalizable type, recursive owned
layouts involving finalizable types, polymorphic finalizable components, defined assignment
involving finalizable types, and named EXIT/CYCLE or GOTO
with live finalizable locals. DEALLOCATE STAT/ERRMSG with finalizable objects is
also unsupported, as are MOVE_ALLOC STAT/ERRMSG for finalizable arrays.
Run `python tests/test_finalization.py`, `python tests/test_array_finalization.py`,
and `python tests/test_owned_finalization.py`;
standalone examples are `tests/programs/xscalar_final.f90` and
`tests/programs/xarray_final.f90` and `tests/programs/xowned_final.f90`.

### Arrays of derived type

A scalar derived component selected across a record array, such as `xx%date`,
is a view of the existing component storage with the parent's shape and
subscripts. This includes nested selections, sections, vector subscripts,
ELEMENTAL calls, and ordinary array dummy arguments, including writable views.
Views can also be assignment targets: `dt%date = dates`,
`dt(2:)%date = dt(:n-1)%date`, or `dt(indices)%date = dates`. Array values,
scalar broadcasts and scalar element assignments use the existing intrinsic
assignment rules. This supports fixed, runtime-allocated and pointer parents,
nested paths, ranks through 15, and the existing numeric, LOGICAL, CHARACTER
and fixed array component layouts. Sibling components and their initialization
state are preserved, and INTENT(IN) components remain read-only. The view does
not own the parent's allocations. See `tests/test_derived_component_views.py`
and `tests/test_component_array_assignment.py`.

Ordinary scalar derived-valued function results can be assigned to array
elements, used as scalar broadcasts, or included in array constructors. Each
function call executes once into a scalar temporary before its components are
copied, including nested scalar, CHARACTER and fixed array components within
the existing assignment limits. Scalar records can also copy individual array
elements. Nested calls to procedures with derived-array dummies retain their
source argument count through component flattening, including generic reductions
inside CHARACTER consumers, keyword/OPTIONAL arguments and ELEMENTAL results.
Module functions may use either their function name or an explicit `RESULT`
variable for the returned value, including derived-type and intrinsic array
results. Typed function headers, renamed/re-exported USE association and
separate `.rmod` compilation preserve the result identity.
See `tests/test_scalar_record_results.py` and
`tests/test_module_function_results.py`.

Derived array constructors concatenate scalar and array-valued items, including
mixed or nested constructors, typed empty constructors, sections, vector
selections and function results. Each array item contributes its records in
column-major order, with source ranks through 15 and fixed or runtime extents.
Results work in assignment, read-only procedure arguments, output, inquiries and
PACK; overlapping assignment captures the RHS before writing the destination.
Nested scalar components and fixed array components retain their record layout.
Known extents remain fixed; dynamic sizes use allocated temporary storage. Run
`python tests/test_derived_constructors.py` for interpreter/native O0/O2 and
gfortran comparisons. Owned ALLOCATABLE components and derived constructor
implied-DOs remain unsupported. Negative-stride sections currently require
explicit lower and upper bounds, as in derived PACK. Other component assignment
and inquiry limitations described in this section still apply.

Derived arrays use separate component arrays with a shared shape, supporting
fixed and allocatable storage, ranks through 15, component defaults, element
access such as `x(i)%id`, constructors, assignment, and list-directed/formatted
output in element order. Array-valued functions execute once and pass their
component result arrays through a hidden subroutine call. This preserves side
effects in ordinary functions and also works for PURE functions. Components may
be numeric, LOGICAL and fixed-length CHARACTER scalars, fixed-size arrays of
those (`a(i)%pos(2)`, `a(i)%pos`, `a(i)%tag(1:2)`), and scalars of a derived type
with such components (`a(i)%center%x`, `a%center%x`), with their defaults.
Output also accepts derived-array rows, columns, triplets and Cartesian vector
selections, including runtime bounds/strides and ranks through 15. Each selected
record transfers its components before the next record; fixed array components
transfer in array element order. Subscript expressions are captured once, and
unselected elements are not read. See `tests/test_derived_section_output.py`.
An element can be passed to a scalar derived dummy (`call bump(a(i), 1.0)`) and
can call type-bound procedures (`call a(i)%move(dx)`, `a(i)%norm()`); the element
is copied into a temporary object for the call and, for a subroutine, copied
back afterwards. Explicit arguments of bound calls also accept derived-array
elements, including renamed bindings, keyword arguments, and calls whose receiver
is itself an array element. Subscripts are evaluated once before copying the
selected element. See `tests/programs/xbound_derived_element_actual.f90`.
Bound procedure calls also associate positional and keyword derived-array
arguments before splitting their component storage, including renamed bindings,
sections and omitted or forwarded OPTIONAL arrays. The implicit receiver is not
an explicit keyword argument. See
`tests/programs/xbound_derived_array_keywords.f90`.
Derived-array operations are lowered in SELECT CASE selectors, case bodies and
CASE DEFAULT, including nested selections and early returns from array-valued
functions. See `tests/programs/xderived_array_select_case.f90`.
Numeric, LOGICAL and fixed-length CHARACTER ALLOCATABLE array components also
work. Each record element owns a separate descriptor holding allocation status, extents, lower bounds,
data and initialization flags. Fixed and runtime-sized parent arrays, component
ranks through 15, direct component reads/writes, ALLOCATE/DEALLOCATE, scalar
procedure calls, READ, RANDOM_NUMBER and MOVE_ALLOC are covered. Scalar-element
intrinsic assignment deep-copies the components; changing or deallocating the
copy does not affect the source. Component allocations are released when their
owning outer array is deallocated or leaves scope. Passing array dummies does
not transfer ownership away from the caller. Character components retain their
Fortran element extents while owned buffers count character codes; copying and
deallocation preserve independent storage. See
`tests/test_character_record_allocations.py`. Operations on individual components
copy back only that component, so allocating or deallocating multiple components
of one parent in a statement does not overwrite sibling allocations.

The representation currently copies owned component storage when materializing
a scalar element. Whole-array and section assignment to conforming record arrays
now deep-copy through a temporary before changing destination allocations. This
preserves overlapping sources, scalar broadcasting, empty arrays, and independent
component ownership. Automatic reallocation of the outer destination array is
not yet implemented by this path; its storage must already have the required shape.

ELEMENTAL functions returning records with owned components support scalar
broadcasting and constant rank-one array arguments/results. Each scalar call
creates independent result components. ELEMENTAL subroutines accept record-array
actuals, including writable elements and rank-one intrinsic array arguments;
scalar numeric and record expressions are evaluated once, and array conformance
is checked.
See `tests/programs/xowned_record_elemental.f90` and
`tests/test_owned_record_array_operations.py`.

INTENT(OUT) array dummies release their existing owned components on entry and
apply component defaults. Components without defaults become undefined. This
includes nested scalar components, fixed array defaults, optional and forwarded
dummies, empty arrays and sections, and parent ranks through 15. Allocatable
INTENT(OUT) parent arrays release both their components and their outer storage,
and enter the procedure unallocated. Nonallocatable dummies preserve the caller's
array bounds and storage. Scalar and array FINAL procedures are described above.
See `tests/test_owned_record_intent_out.py` and
`tests/programs/xowned_record_intent_out.f90`.

Array constructors of records with ALLOCATABLE components, general array-valued function results of these types,
deferred-length CHARACTER allocatable components, POINTER components,
structure constructors of containers whose array elements own allocations, and further arrays of
descriptor-owning containers remain unsupported. Array-valued ELEMENTAL functions
with intrinsic results and owned-record arguments also remain unsupported.
Scalar calls to ELEMENTAL procedures continue to support derived actuals with
ALLOCATABLE components and fixed array components, including scalar array elements.
Their generated component interface preserves ordinary array arguments and purity.
Intrinsic assignment between scalar container variables deep-copies their owned
record arrays, including intrinsic allocations owned by those records.
Default output of records with ALLOCATABLE components requires defined I/O,
as in Fortran. Run `python tests/test_allocatable_record_arrays.py` for
interpreter/native O0/O2 checks against gfortran.

Derived-type pointer arrays support ranks through 15 when their flattened
components are scalar numeric or LOGICAL fields, including nested scalar
records. Component descriptors preserve target identity, initialization masks,
lower bounds and section strides. Fixed or ALLOCATABLE TARGET arrays and array components of
scalar TARGET objects, pointer-to-pointer association, write-through updates,
NULLIFY, ASSOCIATED (including keywords), and local subroutine POINTER dummies
are covered by `tests/test_derived_array_pointers.py`. CHARACTER, array-valued
and owned ALLOCATABLE components in the pointed-to element type remain outside
this subset.

A derived pointer array can also borrow a whole derived-array allocation owned
by a scalar element of a TARGET record array, as in `p => v(i)%xx`. Its
component descriptors alias the owner's data and initialization masks directly;
there is no temporary array copy. This includes runtime extents, zero-size
allocations, lower bounds, ranks through 15, nested scalar records, ASSOCIATED
identity checks, and associations returned through local POINTER dummies.
TARGET is required on the parent object, and escaping associations to local
owned targets are rejected. Binding to an unallocated component is a runtime
error. Matching-shape component updates retain existing data buffers and masks,
so valid existing pointers continue to refer to the updated allocation.
`tests/test_owned_record_pointers.py` checks interpreter/native O0/O2 against
gfortran. Direct subscripted or section targets of the owned component remain unsupported; sections can be
taken from an already associated pointer.

Polymorphic arrays, saved arrays, derived PARAMETER arrays
and a separate-object record ABI remain unsupported. See the finalization
section above for supported FINAL procedures and remaining restrictions.
`tests/programs/xset_alloc_derived.f90` and `tests/185_allocated_derived_concat.f90` exercise empty
and nonempty allocated inputs and replacement of an allocated output.
Run `python tests/test_allocated_array_functions.py` for interpreter/native
O0/O2 checks and gfortran comparisons across element types.
Run `python tests/test_heap_allocations.py` and
`python tests/test_runtime_allocatables.py` for interpreter/native O0/O2 checks
and gfortran comparisons.

### Type-bound procedures and generics

Type-bound procedures may carry PUBLIC, PRIVATE, PASS and NON_OVERRIDABLE.
`generic :: scale => scale_r, scale_i` selects a specific binding by the types of
the actual arguments, as a generic interface does, and `generic :: operator(+) =>
add` and `generic :: assignment(=) => assign` bind an operator or assignment to
the type. `interface assignment(=)` with two-argument subroutines defines
assignment to a whole variable; assignment to an element or a component still
uses intrinsic assignment. DEFERRED bindings are supported as described below;
NOPASS remains unsupported.
Run `python tests/test_derived_type_features.py`.

### Type extension

`type, extends(shape) :: circle` inherits the components and type-bound
procedures of `shape`; the extension's bindings override the parent's. The parent
component `c%shape` names the inherited part, so `c%shape%x` is `c%x` and
`s = c%shape` copies it. An extended object can be passed to a `type(shape)`
dummy when `shape` is concrete, or to a `class(shape)` dummy, and calls its
inherited bindings. Scalar CLASS dummy
calls support overridden binding dispatch as described below. ABSTRACT types
and DEFERRED bindings are supported. Local scalar polymorphic ALLOCATABLE
objects and scalar SELECT TYPE are supported as described below; unlimited
polymorphic scalar procedure dummies are also supported. CLASS(*) allocation
remains unsupported.

Structure constructors accept an explicit parent component, positionally or
by keyword, such as `child(base(4),20)` and `child(base=base(4),extra=20)`.
Inherited fields may alternatively be supplied individually in declaration
order or by keyword. Nested extensions, parent-component views, default
initialization, scalar declaration initializers, procedure arguments and
`ALLOCATE(SOURCE=...)` use the same constructor normalization. Parent-valued
function expressions are evaluated once and copied intrinsically, including
allocatable component state and values; overloaded assignment is not invoked
for the compiler's temporary. Existing record-array and component storage
limitations still apply. Run `python tests/test_inherited_constructors.py`;
the standalone example is `tests/programs/xinherited_constructor.f90`.

### ABSTRACT types and DEFERRED bindings

Fortran 2003 abstract types declare contracts which concrete extensions must
implement. Deferred bindings use an explicit interface and the default first
passed-object dummy:

```fortran
type, abstract :: shape
contains
   procedure(area_interface), deferred :: area
end type

abstract interface
   real function area_interface(self) result(value)
      import :: shape
      class(shape), intent(in) :: self
   end function
end interface
```

A concrete extension supplies `procedure :: area => circle_area`, with a scalar
`class(circle)` first dummy. Abstract intermediate extensions may leave bindings
deferred; concrete extensions must implement every remaining deferred binding.
Inherited concrete implementations can satisfy a contract. Implementations can
be functions or subroutines, and calls through `class(shape)` use the concrete
actual's override. Ordinary bindings implemented in the abstract parent may
call deferred bindings through `self`.

The compiler rejects nonpolymorphic objects, components, constructors and TYPEOF
objects of abstract type. CLASS dummies remain valid. SELECT TYPE accepts
`class is(shape)` but rejects `type is(shape)` when `shape` is abstract.
Deferred bindings outside abstract types, unknown interfaces, and attempts to
use an abstract interface as an implementation are diagnosed.

Contract checks compare function/subroutine category, argument count and names
(except the passed-object name), data types/kinds/ranks, INTENT and supported
OPTIONAL/POINTER/ALLOCATABLE/VALUE/CONTIGUOUS attributes, array shape categories,
constant explicit extents, and result characteristics. Overrides must preserve
PURE/SIMPLE requirements and ELEMENTAL status. NON_OVERRIDABLE inherited
bindings cannot be overridden in these hierarchies. Named constants used for
kinds are resolved in each procedure's scope. Interface IMPORT declarations,
USE aliases, private interfaces, and `.rmod` module compilation are supported.
Contract errors report the binding declaration's source line. Standard modes
before `-std=f2003` reject ABSTRACT types and DEFERRED bindings.

This builds on scalar type specialization, with the existing limits on actual
arguments, storage and results. NOPASS, named PASS arguments, parameterized
derived types remain unsupported. Scalar CLASS(*) dummies are described under
SELECT TYPE. Local scalar polymorphic
allocation is supported as described below.
Procedure-dummy interface comparison and nonconstant specification-expression
compatibility remain limited; this is not complete validation of every Fortran
procedure characteristic.

Run `python tests/test_abstract_types.py`; the standalone example is
`tests/programs/xabstract_shapes.f90`.

### Scalar polymorphic ALLOCATABLE components

Scalar derived objects, including nested scalar containers, can own
`CLASS(parent), ALLOCATABLE` scalar components. Their dynamic types may extend
an ABSTRACT parent. Typed allocation, SOURCE= and MOLD=, ALLOCATED, DEALLOCATE,
SELECT TYPE associate names, scalar-valued dynamic bindings and ordinary CLASS
procedure forwarding use the existing polymorphic allocation machinery.

```fortran
type container
   class(shape), allocatable :: item
end type
type(container) :: box
allocate(circle :: box%item)
print *, box%item%area()
deallocate(box%item)
```

SOURCE= can copy a concrete object, another runtime polymorphic component, or
an ordinary CLASS dummy specialized to its concrete type. Concrete types can
be defined in later modules; source USE ONLY and privacy checks remain in
force. INTENT(IN) on the owner protects its components, and INTENT(OUT)
releases previous allocations and applies defaults. Local ownership follows
scope lifetime. Simple owner assignment makes independent copies of supported
payload storage. SOURCE= also copies payloads with owned intrinsic arrays;
owner assignment of payloads that themselves own allocations retains the
existing record-slot copy restrictions.

Storage is a private type tag plus concrete allocation descriptors inside the
owner, using the concrete types known in the compilation source group. It is
not a public cross-compiler descriptor ABI. Supported array-valued bindings
use the existing function-result lowering. Function-name result subscripts
refer to the local result array rather than recursively invoking the function.
An initialized input array can receive a binding's array result. Generated
dispatch helpers preserve the enclosing procedure's current state rather than
applying its INTENT(OUT) semantics a second time. Actual procedure entry still
undefines output storage, releases output allocations, and applies derived-type
defaults. Polymorphic component arrays,
CLASS(*) components, payload layouts containing further polymorphic allocatable
components (including recursive types), and owner constructors supplying the
polymorphic component remain unsupported.
Access through an unallocated component reports a runtime error.

Run `python tests/test_polymorphic_components.py`. The basic example is
`tests/programs/xpolymorphic_allocatable_component.f90`;
`tests/programs/xpolymorphic_component_array_result.f90` prints `2, 4`.
`tests/programs/xpolymorphic_component_array_self_assignment.f90` also prints
`2, 4`, with the result replacing its input array.

### Scalar polymorphic ALLOCATABLE objects

Local scalar `CLASS(type), ALLOCATABLE` objects can change dynamic type at
runtime. The declared type may be ABSTRACT. Typed ALLOCATE requires a concrete
extension of the declared type; untyped ALLOCATE uses the declared type when
that type is concrete.

```fortran
class(shape), allocatable :: x
allocate(circle :: x)
print *, allocated(x), x%area()
select type (s => x)
type is (circle)
   s%radius = 3.0
end select
deallocate(x)
allocate(rectangle :: x)
print *, x%area()
deallocate(x)
```

`ALLOCATED`, `DEALLOCATE`, exact TYPE IS guards, most-specific CLASS IS guards,
and CLASS DEFAULT are supported. Components can be assigned through guards;
overridden functions and subroutines see the current dynamic object. Ordinary
scalar CLASS dummy procedures can receive an allocated object, including
forwarding through other procedures. Runtime-dependent IF/DO WHILE conditions
are evaluated at their original control points. Reallocation reapplies default
component initialization. Invalid double allocation, deallocation, and access
to an unallocated object stop with a runtime error. Compile-time allocation diagnostics retain source
locations.

Internally each object has a runtime integer type tag and concrete allocation
descriptors. Only the selected concrete storage is allocated. Runtime branches
call private concrete procedure specializations; this does not define a public
polymorphic descriptor ABI or runtime vtable ABI. Plain record element arguments
borrow primitive component storage directly, preserving writes and avoiding
reads of unrelated undefined components.

`ALLOCATE(x, SOURCE=value)` allocates with the source's dynamic type and copies
its value, including supported owned allocatable components. A constructor
SOURCE expression is evaluated once even with multiple allocation objects.
`ALLOCATE(x, MOLD=value)` selects the type without reading component values;
default initialization applies to the new object. An unallocated polymorphic
MOLD with a concrete declared type uses that declared type.

Intrinsic assignment to an allocatable polymorphic scalar allocates an unset
destination, retains storage when its dynamic type matches, or deallocates and
reallocates for a different dynamic type. Copies are independent, self-assignment
preserves the object, and constructor expressions are evaluated before replacing
the destination (so they may call methods of the original object).

```fortran
allocate(x, source=circle(radius=4.0))
allocate(y, mold=x)  ! circle with default component values
x = rectangle(width=2.0, height=7.0)
y = x              ! y becomes a rectangle with a copy of x's value
```

Sources currently support scalar concrete variables, constructors, and the
runtime polymorphic locals described here; arbitrary derived-valued function
results and component/array-element sources are not yet supported by this
allocation path. This implementation does not support polymorphic arrays,
CLASS(*), ALLOCATABLE polymorphic results, OPTIONAL polymorphic allocatable
dummy arguments, saved or module
polymorphic objects, STAT=/ERRMSG= on polymorphic allocation, whole-object
assignment to SELECT TYPE associate names, or runtime polymorphic values in
counted DO bounds.
Existing derived-type component, result and procedure-interface restrictions
still apply. POINTER association and CLASSOF declarations do not acquire
runtime polymorphic descriptor support from this feature.

Run `python tests/test_polymorphic_allocatable.py`. The standalone example is
`tests/programs/xpolymorphic_allocatable.f90`; allocation-source and assignment
examples are in `tests/programs/xpolymorphic_source.f90`.

### Scalar polymorphic allocatable dummy arguments

Required scalar `CLASS(type), ALLOCATABLE` dummy arguments are supported in
specific Fortran subroutines and functions, including ABSTRACT declared types.
The actual must be a scalar polymorphic allocatable variable with the same
declared type as the dummy. It may be unallocated on entry.

```fortran
subroutine make_shape(x)
class(shape), allocatable, intent(out) :: x
! An allocated actual has already been deallocated on entry.
allocate(circle :: x)
end subroutine

subroutine inspect(x)
class(shape), allocatable, intent(in) :: x
if (allocated(x)) print *, x%area()
end subroutine
```

`INTENT(IN)` preserves allocation status and dynamic type and permits inquiries,
SELECT TYPE and read-only method calls. Allocation, deallocation, assignment,
and association with writable dummies are rejected. `INTENT(OUT)` deallocates
an allocated actual on entry, including supported owned components; the
procedure can return a new allocation or leave the actual unallocated.
`INTENT(INOUT)` and omitted INTENT preserve the incoming object and allow
allocation, deallocation, dynamic-type changes and intrinsic assignment.
The caller sees the returned allocation status, value and dynamic type.

Forwarding, positional/keyword arguments, multiple descriptor arguments,
separately compiled `.rmod` modules, pure read-only functions, and impure
functions that replace allocations are supported. Existing PURE constraints
still reject polymorphic INTENT(OUT) dummies. Borrowed concrete storage is not
automatically deallocated when the called procedure returns.

The private lowering passes a mutable type tag and borrowed concrete array
descriptors, using the existing allocation machinery. This does not establish
a public polymorphic descriptor ABI. OPTIONAL descriptors, polymorphic arrays,
CLASS(*), generic selection for these descriptor interfaces, descriptor
arguments in type-bound calls, recursive allocation-call cycles, and narrowed
SELECT TYPE views with a different descriptor layout remain unsupported.
Existing record storage and result restrictions still apply.

Run `python tests/test_polymorphic_allocatable_dummies.py`. The standalone
factory example is `tests/programs/xpolymorphic_allocatable_dummies.f90`.

### Scalar polymorphic POINTER objects

Local scalar `CLASS(parent), POINTER` variables can associate with compatible
concrete scalar TARGET variables, including targets in a module. Binding calls
select the target's dynamic type, including inherited, overridden and deferred
bindings implemented by concrete extensions. Calls and component assignments
borrow target storage; association and NULLIFY never copy or deallocate it.

```fortran
class(shape), pointer :: p
type(circle), target :: c
p => c
print *, p%area()
```

Runtime reassociation in branches and loops, pointer-to-pointer association,
ASSOCIATED (with optional TARGET and keyword arguments), NULLIFY, SELECT TYPE,
and forwarding to ordinary nonpointer CLASS dummies are supported. Pointer
assignment preserves the selected target independently of later reassociation
of the source pointer. SELECT TYPE associate names likewise retain their
selected target until the construct ends. Guard views retain their declared
static type for generic selection while CLASS binding calls use dynamic type.
Separate POINTER and TARGET attribute statements are accepted. Module types
and bindings can come from separately compiled `.rmod` files. Unassociated
binding/component access reports a runtime error.

This implementation uses private target-identity tags over visible concrete
scalar TARGET variables. It does not define a public pointer descriptor ABI.
Polymorphic pointer dummies/results, pointer arrays, CLASS(*) pointers,
ALLOCATE/DEALLOCATE through these pointers, allocatable/component/array-element
targets, saved or module pointer objects, and whole-object assignment through
polymorphic pointers remain unsupported. Internal procedures retain their
existing host-capture limits.

Run `python tests/test_polymorphic_pointers.py`. A standalone example is
`tests/programs/xpolymorphic_pointer.f90`.

### Scalar polymorphic POINTER components

Records can contain scalar `CLASS(parent), POINTER` components. The
owners can be scalars or fixed/runtime-allocated record arrays through rank 15,
including components nested in ordinary scalar records. Module-defined owner
types, USE aliases and separately compiled `.rmod` interfaces are supported.

```fortran
type entry
   class(shape), pointer :: object => null()
end type
type(entry) :: list(2)
type(circle), target :: c
list(1)%object => c
print *, associated(list(1)%object), list(1)%object%area()
nullify(list(1)%object)
```

Each owner element keeps its own association. Reassociation, pointer-to-pointer
association, ASSOCIATED/NULLIFY, scalar target-component writes and type-bound
function/subroutine calls reuse the standalone polymorphic pointer machinery.
Owner assignment copies associations without copying targets. Scalar owner
subscripts are captured once per reference, and loop conditions reload the
current association. Destroying an owner never destroys its borrowed target.

Owners can also be scalar or array dummy arguments to specific procedures with
known explicit interfaces, including forwarding, keyword calls and scalar-valued
functions.
INTENT(IN) prevents changing the component's association while allowing an
impure procedure to change its target. INTENT(INOUT) preserves reassociation in
the caller; INTENT(OUT) applies the owner's default initialization on entry.
Aliased components continue to refer to the same target across these calls.

Owners also support scalar type-bound function/subroutine calls, including
indexed and nested receivers, inherited methods, overrides, calls through
`self`, and explicit parent calls. Named type-bound and module generics select
specifics by type, kind, rank and keyword association; OPTIONAL arguments and
private specifics behind public generics are supported. CLASS owner dummies
keep their declared type for generic selection and their concrete type for
binding dispatch, so forwarding through a parent interface still selects the
child's override. Separate `.rmod`
compilation preserves these interfaces.

Associations to concrete scalar module TARGETs and saved local TARGETs now
survive procedure return. Saved targets with scalar components retain their
default/declaration initialization and share storage across call specializations;
constructor initializers retain local named constants. Owners can read, modify,
copy and nullify these associations after return. Private module targets remain
private, initialization checks follow the target, and separate module compilation
preserves storage identity. INTENT(OUT) resets the owner without destroying its
saved target.

Owners themselves can also be module variables or SAVE variables, including
fixed-size owner arrays through rank 15 and ordinary scalar wrappers. Separate
procedures share their association state, including module USE aliases, private
module owners exposed through public procedures, and separately compiled
modules. SAVE attributes, named SAVE statements and bare SAVE statements work
in procedures; saving one entity in a shared declaration does not save the others.
Saved owner and target storage is shared across call specializations. Owner
bindings, named generics, forwarding and INTENT(OUT) reset retain these semantics.

Persistent owners currently accept associations to explicit SAVE or module
targets. Storing another borrowed target, directly, through an owner dummy, or
by copying an owner, produces a runtime diagnostic. A conditional owner copy
whose association selects a supported static target is allowed. General borrowed
escaping target descriptors remain future work.

These components also support concrete scalar `TYPE(t), ALLOCATABLE, TARGET`
variables, including local, module and saved targets. Allocation uses the existing
record-array descriptor, so aliases borrow the same storage and do not acquire
ownership. Default component initialization, initialization checks, component
arrays, binding calls and separately compiled module targets retain their existing
record-storage behavior. Whole-object assignment allocates an unallocated target
and preserves aliases when assigning to an existing allocation. NULLIFY and
INTENT(OUT) owner reset do not deallocate the target.

Associating with an unallocated target produces a runtime diagnostic. Each
association records a unique allocation identity; dereferencing an alias after its
target is deallocated or reallocated is diagnosed instead of silently selecting
the replacement allocation. Saved/module allocated targets can be returned by
owner factories; ordinary local allocations retain the existing escape restriction.
STAT/ERRMSG report allocation-state errors without changing existing associations.
SOURCE and MOLD allocation support scalar nonpolymorphic model expressions of
the target's declared type, including variables, components, array elements,
ordinary constructors and explicit-interface function results. SOURCE evaluates
its expression once for the allocation list and copies intrinsically, including
deep copies of allocatable components, even when ASSIGNMENT(=) is overloaded.
MOLD supplies type information without reading the model's component values;
allocation applies default initialization and leaves other components undefined.
Failed allocation with STAT preserves the existing target and its aliases.
All objects in these allocation lists must currently be supported scalar targets
of the same type. Constructors with explicit parent parts are supported;
model expressions otherwise retain the existing expression-lowering limits.
MOVE_ALLOC carries the identity and storage to the destination, so existing
aliases follow a TARGET destination and cannot select a later reallocation of
FROM or the allocation formerly owned by TO. Pointer components can escape after
a move to saved/module storage; a moved allocation still owned by an automatic
local retains the existing escape check. Supported pointers to scalar and fixed-array
components, and pointers into supported owned array components, retain their
underlying allocation as well.
Scalar allocatable TARGET dummy descriptors and polymorphic allocated targets
and model expressions remain unsupported.

Scalar function results containing these pointer components are supported,
including explicit RESULT names, implicit function-name results, early RETURN,
null results, nested scalar wrappers and owner-dummy forwarding. Returned
associations preserve target identity, dynamic dispatch and initialization
checks; copying a result does not copy or own its target. Separate compilation
works with module and saved factory targets. At each return, an association to
an automatic or other borrowed target produces a runtime diagnostic: this
implementation currently requires a saved or module target for escaping
results. Temporary borrowed associations are allowed if cleared or replaced
before returning. Array-valued owner results remain unsupported.

Static targets use a shared storage-identity table; automatic targets still use
hidden borrowed TARGET arguments alongside association tags. Neither form copies
or owns targets, and this is not a portable descriptor ABI. Recursive owner
procedures and new automatic local TARGET storage in owner-dummy procedures
remain unsupported, as do runtime polymorphic
owner receivers, shadowed host targets, external calls without a known interface,
constructors with explicit arguments for these owner types, SELECT TYPE on owner
dummies or pointer components,
unlimited/array pointer components and the standalone pointers' unsupported
target/allocation forms. General escaping
owner storage still needs a general persistent component descriptor ABI.

Run `python tests/test_polymorphic_pointer_persistent.py` for static-target tests.
`tests/programs/xpolymorphic_pointer_saved_target.f90` exercises association after
return, shared-target mutation, owner copying and NULLIFY.
Run `python tests/test_polymorphic_pointer_saved_owners.py` for module/SAVE owner
tests. `tests/programs/xpolymorphic_pointer_saved_owner.f90` tests shared module
association state and saved owners across separate calls.
Run `python tests/test_polymorphic_pointer_results.py` for function-result tests.
`tests/programs/xpolymorphic_pointer_result.f90` demonstrates a factory, shared
target mutation, dispatch and a null result.
Run `python tests/test_polymorphic_pointer_allocated_targets.py` for allocated
target tests. `tests/programs/xpolymorphic_pointer_allocated_target.f90` shows
an allocating factory, shared mutation and explicit target deallocation.
Run `python tests/test_polymorphic_pointer_allocation_models.py` for SOURCE/MOLD
tests. `tests/programs/xallocated_target_models.f90` contrasts deep copying with
default initialization.

Run `python tests/test_polymorphic_pointer_components.py`. The reduced example
`tests/programs/xpolymorphic_pointer_array_component.f90` prints `T 7`.
`tests/programs/xpolymorphic_pointer_component_dummy.f90` tests procedure calls,
target mutation, reassociation and default initialization on INTENT(OUT).
`tests/programs/xpolymorphic_pointer_owner_bindings.f90` tests owner bindings,
overrides and generics. Run `python tests/test_polymorphic_pointer_owner_bindings.py`
for the additional interface regressions.

### Scalar dynamic binding dispatch

Ordinary passed-object bindings dispatch through scalar `CLASS(parent)` dummies
according to the actual object's type. Overridden functions and subroutines,
inherited implementations, calls through `self`, forwarding through specific
procedures, multiple CLASS arguments, keyword/OPTIONAL arguments, and separate
`.rmod` module compilation are supported. A bound procedure may itself use
SELECT TYPE or forward its passed object to another polymorphic procedure.

```fortran
subroutine show(x)
class(shape), intent(in) :: x
print *, x%area()  ! circle actual calls circle's overridden area
end subroutine
```

The passed object's declared type still controls component visibility and
ordinary generic resolution; knowing the actual type does not expose extra
components outside a suitable guard. Writable bindings update caller storage.
INTENT(IN) objects cannot be passed to INTENT(OUT/INOUT) bindings. INTENT(OUT)
default initialization uses the complete actual type. An explicit parent call
such as `self%shape%area()` selects the parent's implementation and treats that
parent component as nonpolymorphic, so it does not call the override again.

Dispatch uses private call-site type specialization with borrowed component
views, rather than a public runtime vtable/descriptor ABI. It currently requires
simple scalar variable actuals and the default first passed-object dummy.
Polymorphic arrays, CLASS(*), CLASSOF dispatch,
POINTER/VALUE CLASS dummies, and generic interfaces containing
specialized CLASS specifics remain unsupported. NOPASS and named PASS arguments
retain their existing limitations.
Local scalar polymorphic pointer receivers can borrow the concrete targets
described in [scalar polymorphic POINTER objects](#scalar-polymorphic-pointer-objects).
Scalar ALLOCATABLE CLASS dummies use the runtime descriptor path described above.
Other record storage and result forms retain existing backend restrictions.

Run `python tests/test_dynamic_dispatch.py`; the standalone example is
`tests/programs/xdynamic_dispatch.f90`.

### SELECT TYPE

Fortran 2003 scalar `SELECT TYPE` is supported for `CLASS(parent)` dummy
arguments of specific Fortran procedures and local scalar polymorphic
allocatables and pointer objects described above. `TYPE IS(child)` matches exactly;
`CLASS IS(parent)` matches that type and its extensions. Exact matches take
precedence, then the most specific matching CLASS guard, irrespective of
source order. `CLASS DEFAULT` handles remaining types; without it, a selector
with no matching guard executes no branch.

```fortran
subroutine change(x)
class(base), intent(inout) :: x
select type (v => x)
type is (child)
   v%extra = v%extra + 1
class is (base)
   print *, v%value
class default
   print *, "other type"
end select
end subroutine
```

Associate names borrow the caller's storage, including supported allocatable
components. Without an associate name, the selector itself has the guard's
type within that branch. Outside the selection it retains its declared type,
so generic resolution still uses the declared parent. INTENT(IN) assignments
are rejected, and INTENT(OUT) default initialization uses the complete dynamic
type once. Keyword/OPTIONAL association, multiple selectors, nested selections,
forwarding to specific procedures, scalar intrinsic function results and
separately compiled `.rmod` modules are supported. Named constructs and local
BLOCK declarations are accepted. Guard and component diagnostics include
source locations; SELECT TYPE requires `-std=f2003` or later.

Procedure calls use private specializations, while local polymorphic allocatables
select their concrete storage at runtime. Neither defines a public runtime
polymorphic ABI. Scalar `CLASS(*)` procedure dummies also accept intrinsic
actuals: INTEGER kinds 1/2/4/8, REAL and COMPLEX kinds 4/8, default LOGICAL,
and CHARACTER. Intrinsic `TYPE IS` guards match type and kind; CHARACTER
guards require `LEN=*` and retain the actual length in the narrowed view.
Named kind constants follow USE and host association. Supported scalar
intrinsic expressions can supply read-only arguments. Writable variable
actuals and guard aliases share caller storage; INTENT(IN) is retained.
Derived exact/ancestor guards and CLASS DEFAULT can occur in the same selection.

Array selectors, CLASS(*) allocation descriptors, POINTER/VALUE CLASS dummies,
and generic interfaces containing specialized CLASS specifics remain
unsupported. Derived-type actuals retain their scalar-variable restriction.
Ordinary scalar binding calls through narrowed CLASS views use
the actual type's override; exact TYPE IS views use that concrete type's binding.
Scalar ALLOCATABLE selectors follow the runtime descriptor path described above.
Other record storage forms retain the existing derived-type restrictions.

Run `python tests/test_select_type.py` and
`python tests/test_select_type_intrinsic.py`. Standalone examples are
`tests/programs/xselect_type.f90` and
`tests/programs/xselect_type_intrinsic.f90`.

### TYPEOF and CLASSOF declarations

These follow the declaration rules in [J3/24-007, section 7.3.2.1](https://j3-fortran.org/doc/year/24/24-007.pdf).

Fortran 2023 `typeof(object)` declares a nonpolymorphic entity with the declared
type and type parameters of `object`. Its value, rank, bounds and attributes are
not copied. For example:

```fortran
real(kind=dp) :: x(10)
typeof(x) :: scalar, work(20)
```

The reference can name a supported intrinsic or derived-type object, an array
element or a component. Chained declarations, declaration attributes, initialized
objects, component definitions, BLOCK locals, host association and USE association
(including renaming and private type names or kind constants behind an exported object) are
supported. The reference's type must already be declared or established by
implicit typing. Type inspection does not require an initialized value.

CHARACTER lengths follow the referenced object's type parameters: assumed length
becomes the referenced object's actual length, while deferred length stays
deferred. An OPTIONAL reference cannot have assumed or deferred type parameters.
The compiler's existing restrictions on CHARACTER storage still apply.

`classof(object)` uses the declared derived type and type parameters, declaring a
polymorphic entity. Its destination must be a dummy argument or have ALLOCATABLE
or POINTER. Supported scalar CLASS dummy-argument cases use the existing derived-type
and type-extension lowering; this does not add dynamic dispatch or polymorphic
allocation. Intrinsic references are rejected. TYPEOF rejects unlimited
polymorphic references; CLASSOF of CLASS(*) is valid Fortran but receives an
explicit unsupported diagnostic. Parameterized derived types remain unsupported.

These specifiers are accepted only in type declarations and component definitions,
not in function prefixes, ALLOCATE type specifications or array constructors.
Standard modes older than `-std=f2023` reject them with source locations.
Run `python tests/test_referenced_types.py`. Examples are
`tests/programs/xtypeof.f90` and `tests/programs/xclassof.f90`.

### Derived arguments and OPTIONAL

Scalar OPTIONAL arguments retain their positions during derived-type lowering,
including calls to ordinary procedures, bound procedures and functions returning
a derived value. An unrelated derived type in the same source group no longer
makes omitted arguments required. `tests/programs/xstr_int.f90` and
`tests/test_derived_optional.py` cover omitted and reordered arguments, forwarding
absent dummies, and component expansion around optional arguments.

A derived-type dummy argument may itself be OPTIONAL, as in
`type(stretch_param), intent(in), optional :: par`. Its components are passed as
OPTIONAL arguments, PRESENT(par) tests them, and omitting it omits them all.

Derived-type lowering respects local array declarations and host association:
array subscripts are not associated with a same-named procedure's arguments.
Calls that do not use derived arguments or results pass through without argument
rewriting. `tests/programs/xivec_shadow.f90` and `tests/test_derived_shadowing.py` cover this
behavior, including comparisons with and without an unrelated derived type.

## Procedures

### C interoperability

Native compilation supports a subset of Fortran 2003 C interoperability and
Fortran 2018 assumed-shape C descriptors:

- `USE ISO_C_BINDING`, including `INTRINSIC`, `ONLY`, and renaming. Supported
  kind constants are `C_SIGNED_CHAR`, `C_SHORT`, `C_INT`, `C_LONG`, `C_LONG_LONG`,
  `C_INT8_T`, `C_INT16_T`, `C_INT32_T`, `C_INT64_T`, `C_INT_LEAST8_T` through
  `C_INT_LEAST64_T`, `C_SIZE_T`, `C_INTPTR_T`, `C_FLOAT`, and `C_DOUBLE`.
  Values reflect the compiler host ABI; in particular `C_LONG` is 4 on Windows
  and 8 on supported 64-bit Unix hosts.
- Scalar INTEGER kinds 1, 2, 4, 8 and REAL kinds 4, 8 in `BIND(C)` functions
  and subroutines. Without NAME, the C label is the lowercase Fortran procedure
  name. `BIND(C, NAME="label")` accepts a literal, case-sensitive C symbol.
  Both RESULT/BIND clause orders and no-argument subroutines are accepted.
- Explicit-shape INTEGER and REAL array dummies, with any supported rank,
  in both call directions. Extents may depend on input scalar INTEGER arguments,
  including `VALUE` arguments. Nondefault lower bounds and zero extents are
  supported. C receives a pointer to contiguous elements in Fortran column-major
  order, with conversion for each element's declared kind. Array dummies cannot
  have `VALUE`; `INTENT` controls copying and output initialization tracking.
- Assumed-size INTEGER and REAL array dummies such as `x(*)`, `x(3,*)`,
  and `x(0:2,-1:*)`, in both call directions. C exports access C-width elements
  directly, so a separate argument named `n` is never treated as a hidden length.
  Leading dimensions retain their bounds checks; callers are responsible for
  supplying sufficient storage and initialized C input. Bounded sections are
  supported, as are inquiries on known dimensions and `LBOUND` on the last.
  Whole-array operations, `SIZE`/`UBOUND` of the last dimension, and sections
  missing their last upper bound are rejected. C storage can be forwarded to
  another compatible C interface without conversion or ownership transfer.
- Scalar `VALUE` dummies, including the separate `VALUE :: x` statement.
  Changes to a VALUE dummy affect only its local copy, including calls from
  Fortran. Non-VALUE scalar dummies use C pointers and support `INTENT(IN)`,
  `INTENT(OUT)`, `INTENT(INOUT)`, or omitted INTENT.
- Assumed-shape INTEGER and REAL array dummies of ranks 1–15, in both call
  directions, using `CFI_cdesc_t`. Nondefault Fortran lower bounds, zero extents,
  and strided or reversed sections are supported. Descriptors carry extents and
  signed byte strides; C exports access C-width storage directly. Fortran-to-C
  adapters convert private storage while preserving section strides and copy
  output elements back with their initialization flags. Forwarded C storage
  remains a direct view.
- Explicit interfaces to imported C functions/subroutines, with local USE or
  IMPORT declarations for numeric kinds; module procedure exports and module
  dependencies compiled separately using `.rmod` interfaces.
- Opaque scalar `TYPE(C_PTR)` variables, named constants initialized to
  `C_NULL_PTR`, assignment, procedure arguments and function results.
  `BIND(C)` pointer arguments support both `VALUE` (`void *`) and by-reference
  (`void **`) passing. `C_ASSOCIATED` accepts one or two C pointers; two null
  pointers are not associated. Intrinsic keywords, USE renaming, re-exports and
  host association are supported. Pointer arithmetic, direct comparisons and
  numeric assignment conversions are rejected.
- `C_F_POINTER(CPTR, FPTR [, SHAPE, LOWER])` associates numeric Fortran pointers
  with C storage, including scalars and array ranks 1–15. SHAPE and LOWER can be
  rank-one INTEGER array expressions or variables; SHAPE is required exactly
  when FPTR is an array. LOWER is a Fortran 2023 feature and defaults to all ones.
  Null CPTR disassociates FPTR. Zero extents are supported, and shape lengths,
  negative extents and size overflow are checked. Association borrows C storage
  without copying it or taking ownership; the C owner controls its lifetime.
- `C_LOC(X)` for initialized, contiguous numeric TARGET or associated POINTER
  storage. Private Fortran storage initially requires `INTEGER(C_INT64_T)` or
  `REAL(C_DOUBLE)`, whose eight-byte representation matches C. A numeric pointer
  associated by C_F_POINTER can expose any supported C numeric kind. Narrower
  private kinds are diagnosed because their internal representation differs;
  no temporary address is returned. Empty targets are rejected.
  Nonpointer dummy arguments that may be copied by a procedure adapter are
  currently rejected by C_LOC; direct C assumed-shape/assumed-size views are
  supported. This preserves address identity with the caller's storage.

Adapters convert between C storage widths and rfort's private representation;
C callers do not receive rfort's initialization descriptors. Callers must supply
compatible C declarations and use the same architecture and ABI as rfort.
C interoperability requires native code: use `rfort file.f90 --run` rather than
`rfort run file.f90`. Compile C separately and pass its object to rfort for linking.

The runnable example `tests/c_interop/scalar.f90` exports
`int AddMixed(int, float)` and `void bump_values(int *, double *)`.
On Windows with Clang and rfort on PATH:

```cmd
clang --target=x86_64-pc-windows-msvc -c tests\c_interop\caller.c -o target\c-caller.obj
rfort tests\c_interop\scalar.f90 target\c-caller.obj -o target\c-example.exe --run
```

The C main checks both calls and exits zero on success. A matrix example is
provided in `tests/c_interop/arrays.f90` and `tests/c_interop/array_caller.c`;
compile and link those files in the same way. The assumed-size example in
`tests/c_interop/assumed_size.f90` and `assumed_size_caller.c` tests an input
array function, output array subroutine, and zero-count calls. On Linux/macOS, compile
`caller.c` with the host C compiler and pass its `.o` file to rfort instead.
Run `python tests/test_c_interop.py` for native C-to-Fortran and Fortran-to-C tests
at O0/O2. The tests require Clang on Windows, or a host C compiler on Unix.
The current implementation has been verified on Windows x64.

For descriptor interfaces, compile C with rfortran's own
`include/ISO_Fortran_binding.h`. The header and embedded runtime provide
`CFI_CDESC_T`, numeric type constants, `CFI_establish`, `CFI_address`, and
`CFI_is_contiguous`. This is rfortran's descriptor ABI; descriptor layouts and
type codes from another compiler are not interchangeable. The initial subset
supports `CFI_attribute_other` only. Descriptor version, rank, type, element size,
extents, stride arithmetic, and null storage are checked before use. A C callee
cannot change the metadata of these nonallocatable, nonpointer descriptors.

The descriptor example is `tests/c_interop/descriptors.f90` with
`tests/c_interop/descriptor_caller.c`:

```cmd
clang --target=x86_64-pc-windows-msvc -I include -c tests\c_interop\descriptor_caller.c -o target\c-descriptor.obj
rfort tests\c_interop\descriptors.f90 target\c-descriptor.obj -o target\c-descriptor.exe --run
python tests/test_c_descriptors.py
```

Plain-pointer array adapters (explicit-shape and assumed-size interfaces) still
require contiguous actual storage. Array function results, assumed-rank C
dummies, the CONTIGUOUS attribute on assumed-shape C dummies, allocation-changing
descriptors, and the remaining CFI routines are not yet supported.
Fortran-to-C adapters assume that C
fully defines any output-only array; C writes cannot be individually tracked.
Pass a bounded section when C accesses only part of an actual array, especially
if its remaining elements are undefined. Assumed-size C exports do not have a
final upper-bound or initialization mask. Exported array bounds support integer literals, input
scalar INTEGER arguments, unary minus, and arithmetic `+`, `-`, `*`, `/`.

Not yet implemented: CHARACTER, LOGICAL,
COMPLEX, derived-type layouts, global variable bindings, OPTIONAL/POINTER/
ALLOCATABLE dummies, C_PTR arrays/components, `C_FUNPTR`, `C_FUNLOC`,
`C_F_PROCPOINTER`, callbacks, and other ISO_C_BINDING entities. Acceptance of a
`TYPE, BIND(C)` header does not establish a C-compatible structure layout.

Pointers associated with C storage currently cannot also be associated with
private Fortran pointer storage using `=>`; such mixtures are diagnosed. These
numeric C pointer operations require native compilation. Run
`python tests/test_c_pointers.py` for C buffer sharing, pointer arguments/results,
null checks, numeric kinds, ranks and diagnostics at O0/O2. A runnable buffer
example is `tests/c_interop/pointers.f90` with `pointer_buffer.c`:

```cmd
clang --target=x86_64-pc-windows-msvc -c tests\c_interop\pointer_buffer.c -o target\pointer-buffer.obj
rfort tests\c_interop\pointers.f90 target\pointer-buffer.obj --run
```

### External procedures and separate compilation

External subroutines and functions, outside any module or main program, are
called by reference, as in FORTRAN 77, so they can be compiled separately with
`-c` and linked later. Each one has an entry point, `rfort$name`, that receives
every argument by reference. A caller needs no interface for it, so it may pass
a scalar, a whole array, an array element or an expression, and the callee
applies its own declarations:

- Explicit-shape dummies such as `x(n)` and `a(lda,n)`, with lower bounds if
  given, and assumed-size dummies such as `x(*)`, `a(lda,*)` and `w(0:*)`.
- An array element actual starts the dummy array at that element (sequence
  association), as in `call dscal(n, alpha, a(i,j), 1)`.
- A constant or expression actual is passed in a temporary.
- INTEGER, REAL, DOUBLE PRECISION, COMPLEX and LOGICAL dummies and function
  results. A COMPLEX argument passes its real and imaginary parts as two
  arguments, and a function with a COMPLEX result returns them in two more.
- CHARACTER dummies receive a copy of the actual. A fixed-length dummy that
  the procedure changes is copied back on return; an assumed-length
  `character*(*)` dummy has the actual's length, and the procedure cannot
  change it.

A function is called this way when it is declared EXTERNAL, as in
`double precision ddot` / `external ddot`, or only typed, as FORTRAN 77
allows, or when it is defined in the same compilation with array or CHARACTER
dummies, or may define one of its scalar dummies, which its actual argument
must then reflect. Its references are evaluated
before the statement that contains them, and before every test of a DO WHILE
condition. A subroutine is called this way when it is not defined in the
compilation, or when it has array or CHARACTER dummies.

Element accesses through a dummy are checked against its bounds and against
the end of the actual array, and reads are checked for initialization, as
for local arrays. A procedure entered with an explicit-shape dummy larger than
its actual, or with an array element outside its array, stops with an error.

Module and internal functions that define a scalar dummy, as in `k = k + 1`
in the body of `f(k)`, are called through a subroutine with the result as a
last argument, so that their actual arguments change too. Their dummies must
be declared scalars.

Inside a procedure, elemental intrinsics such as `DBLE`, `ABS`, `MAX` and
`SQRT`, and COMPLEX arithmetic, apply to sections of dummy arrays, as in
`a(k1:k2, j) = s*dconjg(qc(1, 1:n))`.

Automatic arrays of these procedures, such as `DOUBLE PRECISION WORK(MAX(M, N))`
or `COMPLEX*16 T(N)`, are allocated on entry, as in other procedures.

TRANSFER of a REAL array to a COMPLEX array of the same kind may be an actual
argument, as LAPACK's complex xxxRFSX routines pass a REAL workspace with
`TRANSFER(RWORK(1:2*N), (/ (ZERO, ZERO) /), N)`: the callee receives a
temporary COMPLEX array of SIZE elements, made from consecutive pairs of the
source, which it may change without changing the source. TRANSFER is not
otherwise supported.

Not yet supported in these calls: derived-type arguments, array sections, CHARACTER arrays, procedure arguments, assumed-shape dummies
(which need an explicit interface), and whole-array operations on a dummy
inside the procedure. Run `python tests/test_external_calls.py`.

### Named arguments

Calls to procedures defined in the source group support named arguments,
including reordered keywords and positional arguments followed by keywords.
Omitted OPTIONAL arguments can occur before later named arguments, and absent
OPTIONAL values can be forwarded by name. Generic calls associate keywords with
each candidate's dummy names before matching types. Named scalar, supported
array/CHARACTER and type-bound procedure calls use the existing procedure
lowering. Common math intrinsics and RANDOM_NUMBER(HARVEST=...) also accept names;
existing intrinsic keyword support remains available.

INTEGER, REAL, DOUBLE PRECISION and LOGICAL multidimensional expressions passed
to INTENT(IN) array dummies retain their type, kind, rank and active shape. Fixed-shape expressions
use fixed temporaries; runtime-sized expressions use owned heap descriptors.
Comparisons, arithmetic, logical operations, supported elemental calls and
scalar broadcasting are supported through rank 15, including empty arrays and
runtime shape checks. This applies to positional and named calls and optional
dummies. Run `python tests/test_multidimensional_expression_arguments.py` for
interpreter/native O0/O2 checks against gfortran.

Unknown keywords, duplicate arguments, positional arguments after keywords and
missing required arguments are compile errors. Named procedure calls currently
require a known definition in the source group; external interface-only declarations
remain unsupported. Run `python tests/test_named_arguments.py`.

### OPTIONAL arguments

Local OPTIONAL functions and subroutines can be specialized for positional and
named calls, including omitted arguments before later named arguments and
forwarding an absent optional dummy to another optional dummy. PRESENT
and simple logical combinations of presence tests select the applicable branches;
a missing required argument is diagnosed. A reference to an absent dummy that
always executes is diagnosed at compile time; one that may not execute, such as
`if (n > 0) y = f(x, nacf)` with `n` set only when `nacf` is present, stops the
program with an error only if it executes.
Optional functions currently require explicit INTENT(IN) on their arguments.
Recursive calls reuse the specialization for each argument-presence combination.
A separate-object optional-argument ABI remains unsupported. Named calls require
a definition in the source group.

ELEMENTAL functions also support OPTIONAL numeric, LOGICAL, and CHARACTER
inputs, scalar broadcasting, conforming array actuals, and absent-argument
forwarding. Numeric elemental array results can be passed to array dummies.
Specialized presence branches retain source-level parameter-use information
for unused-argument warnings. `tests/144_optional_elemental.f90` tests the
`sgn_real` example with scalar and array calls. INTEGER constant ERROR STOP
codes are supported in native programs and the interpreter, with the code
used as the process exit status. Run `python tests/test_optional_elemental.py`
for these checks and gfortran comparisons.

### CHARACTER arguments

Scalar CHARACTER subroutine arguments support fixed and assumed lengths,
including OPTIONAL inputs and writable variables. Assumed lengths follow each
actual argument; LEN, empty strings and literal/concatenated inputs work. Fixed
lengths currently require an exact match. Expressions require INTENT(IN), while
writable arguments must be variables. Explicit RETURN leaves an expanded
subroutine, including from nested loops, and execution resumes in its caller.
Fixed-length rank-one CHARACTER array subroutine arguments also use call-site
specialization, including assumed-length dummies.
Run `python tests/test_optional_procedures.py`.

Ordinary functions returning numeric or LOGICAL scalars also specialize
read-only CHARACTER scalar inputs, binding fixed or assumed lengths at the
call site. Known definitions in the source group, named and nested calls,
OPTIONAL inputs, empty strings, runtime string contents and early RETURN
are supported with the existing CHARACTER function limits. This covers
`util.f90:assert_eq2`, reproduced by `tests/155_scalar_character_arguments.f90`.
STOP with a CHARACTER literal or named constant prints its message to stderr
and terminates normally with status zero; a default INTEGER literal or named
constant prints its code and uses it as the process exit status. Bare STOP
remains silent, and PURE procedures reject STOP. `tests/156_stop_code.f90`
checks termination with an INTEGER code. Run
`python tests/test_scalar_character_arguments.py` for interpreter/native O0/O2
checks and gfortran comparisons of function results.

Local CHARACTER scalar and rank-one array declarations in specialized subroutines
can use `LEN(text)` of a CHARACTER dummy, including supported constant arithmetic
such as `LEN(text)+2` or `MAX(1,LEN(text))`. Each call binds the length from the
actual argument's declared scalar or element length; the contents can be read
at runtime. Nested calls and BLOCK declarations keep their own lengths.
Specification expressions cannot depend on OPTIONAL dummies. Runtime-dependent
`LEN_TRIM(text)` lengths remain unsupported. `tests/154_dummy_character_lengths.f90`
isolates the local `mod_text` declaration from `util.f90:read_vec_real`.
Run `python tests/test_dummy_character_lengths.py` for interpreter/native O0/O2
checks and gfortran comparisons.

TRIM expressions passed to assumed-length INTENT(IN) CHARACTER dummies retain
their runtime length separately from storage capacity. Their operands are
captured once. Dependent local lengths such as `character(len=len(s)) :: t`
and bounded arithmetic on that length preserve the logical length, including
runtime empty strings and forwarding to another procedure. Constant zero-length
actuals with guarded out-of-range substring references remain a separate
compile-time checking limitation. See
`tests/programs/xtrim_actual_local_length.f90` and
`tests/test_trim_actual_local_length.py`.

### Scalar CHARACTER functions

Ordinary scalar CHARACTER functions defined in the source group now specialize
calls into caller-owned result buffers. Fixed-length and call-site constant
result lengths, including `len(a)` and `len_trim(a)` for constant actuals,
support assignments, concatenation, formatted output, named and nested calls,
early RETURN, OPTIONAL inputs, and procedures with no arguments. Scalar dummies
require explicit INTENT. Writable arguments must be whole variables, and locals
must be explicitly declared.
Constant length evaluation respects fixed dummy lengths, including truncation
of a longer actual argument.
CHARACTER result and local lengths can use `SIZE(array)` and `SIZE(array,dim)`
with actual array shapes known during specialization, including sections and
multidimensional arrays. For example, `join_fixed` can declare its result as
`character(len=size(words)*nlen)` when `nlen` is a constant actual argument.
`tests/182_character_result_size.f90` exercises this case with runtime formatted
internal output.

Deferred-length ALLOCATABLE scalar results use the existing bounded CHARACTER
storage lowering for calls from the main program. Assignment sets the runtime
result length, so concatenation, LEN and assignment to a caller's allocatable
variable preserve that length. Runtime lengths for explicitly sized results,
recursive CHARACTER calls, host captures, and a
separate-object CHARACTER result ABI remain unsupported. Runtime-length results
as actual arguments to other procedures remain outside this subset.

`tests/146_character_functions.f90` reproduces `xchar_func.f90`, including its
allocatable-result function. Run `python tests/test_character_functions.py` for
interpreter/native O0/O2 checks and gfortran comparisons. Unused CHARACTER
function definitions are removed before native result-storage validation;
this allows the `exe_name` definition in `util.f90` to pass that blocker.
GETCWD and GET_COMMAND_ARGUMENT themselves remain unsupported when called.

Scalar CHARACTER function results with lengths determined by the element width
of an array dummy support fixed arrays and runtime-sized sections. Result
specifications respect dummy/local declarations that shadow module entities,
and empty-array branches are specialized before checking their element access.
`tests/programs/xfirst_character.f90`, `tests/203_first_tail.f90`, and
`python tests/test_first_tail.py` cover these cases and numeric empty-array
sentinels.

Scalar CHARACTER results and locals can also have content-dependent lengths,
including `SUM(LEN_TRIM(words))`. These specifications are evaluated once on
procedure entry, and their declared length stays fixed for that invocation.
Storage is bounded by known argument widths and capacities; runtime-sized
sections read only their active elements. LEN, nested assumed-length arguments,
substrings and internal WRITE retain the logical length rather than the storage
capacity. Unbounded heap inputs and unsupported specification expressions still
produce diagnostics. `tests/programs/xjoin_length.f90`, `tests/204_joins.f90` and
`python tests/test_joins.py` cover joins, empty inputs and runtime WRITE overflow.

### CHARACTER array functions

CHARACTER functions can accept read-only arrays and return fixed-shape arrays
of ranks 1 through 15. Calls specialize assumed lengths and shapes from actual
arguments; result lengths and extents may use LEN and SIZE of the dummies.
Inputs and results use local buffers with at most 1024 elements per array.
Literal constructors, typed empty constructors, fixed arrays, constant sections,
named calls, and another CHARACTER array function's result are supported as
actuals. The input arrays need not have equal extents. Fixed-length array dummies
currently require the actual element length to match; explicit-shape dummies
require matching extents. Scalar INTEGER constant actuals can supply result
extents. In this bounded specialization path, runtime-dependent result extents,
nonunit declaration bounds, POINTER/ALLOCATABLE array dummies and deferred-length
array results remain unsupported. Read-only array actuals are copied and must
be initialized.

Non-elemental functions with fixed CHARACTER element lengths can also return
runtime-sized arrays through a caller-owned heap descriptor. This path supports
ALLOCATABLE results, runtime-sized assumed-shape inputs, explicit or assignment
allocation, empty results, and repeated calls without the 1024-element bound.
Arguments require explicit INTENT declarations; OPTIONAL and POINTER dummies
and ALLOCATABLE CHARACTER dummies still use the existing restricted path.
Results can be assigned to allocatable or conforming fixed arrays, printed,
queried, or forwarded to another procedure. Fixed destinations check the shape
and preserve CHARACTER padding/truncation. See
`tests/programs/xunion_character_result.f90` and
`python tests/test_runtime_character_results.py`.

CHARACTER FORALL assignments to fixed rank-one arrays with lower bound one
now use the existing
numeric FORALL snapshot rules: capture all right-hand sides before destination
writes, check bounds, and reject duplicate destinations. Triplets and destination
subscripts must resolve to constants; stable local constants can be used in
subscripts after function specialization. Supported PURE CHARACTER procedure
calls inside FORALL use the same snapshots. Function results support assignment, concatenation,
formatted output, LEN/SIZE inquiries, nested calls, and early RETURN.

`tests/148_character_array_functions.f90` reproduces `xmunge.f90` and the
`munge` function in `util.f90`. Run `python tests/test_character_array_functions.py`
for interpreter/native O0/O2 checks and gfortran comparisons, including empty
arrays, zero element lengths, input sections, and overlapping FORALL assignment.

Functions returning numeric or LOGICAL rank-one arrays can use the same
specialization when they have CHARACTER input dummies. Assumed CHARACTER
lengths bind to each actual argument, including strings read at runtime;
the result extent must still be known at the call site. Read-only rank-one
array inputs, named and nested calls, empty arrays, and early RETURN are
supported with the limits above. This does not add a separate-object CHARACTER
argument ABI or multidimensional array results.
`tests/149_numeric_array_character_arg.f90` reproduces the `position` argument
blocker from `util.f90:combine_vec_mat_real` with a rank-one result.
Run `python tests/test_numeric_array_character_arg.py` for interpreter/native
O0/O2 checks and gfortran comparisons.

Functions with read-only rank-one CHARACTER array inputs can also return
numeric or LOGICAL scalars. This includes assumed element lengths, typed
function headers, named calls, fixed array sections, empty arrays, and CHARACTER
array function results as actuals. Array extents must be known at the call site;
their string contents can be read at runtime. The same input initialization,
rank, and size limits above apply. `tests/150_character_array_scalar_result.f90`
reproduces the CHARACTER dummy blocker in `util.f90:num_unique_char`.
Run `python tests/test_character_array_scalar_result.py` for interpreter/native
O0/O2 checks and gfortran comparisons.

### Array arguments to functions

PURE functions with scalar INTEGER, REAL, DOUBLE PRECISION or LOGICAL results
can take read-only numeric/LOGICAL array arguments with fixed or assumed shapes.
Calls reuse specialized subroutine argument association, so forwarding through
another procedure and runtime-sized arrays use the actual array descriptors.
Numeric assumed-shape inputs support ranks 1 through 15, with extents checked
at runtime as needed. Assumed-shape dummy lower bounds start at 1. Scalar-result
functions with read-only CHARACTER inputs also use subroutine association;
existing CHARACTER storage and extent limits apply.

Whole arrays, empty arrays, constructors, supported sections and array
expressions are accepted. Runtime array expressions are evaluated into temporary
heap storage before association. Array elements are checked for initialization
when read; a SIZE inquiry does not require initialized data. Explicit-shape
numeric function inputs retain the existing fixed-size ABI and its 1024-element
limit. OPTIONAL, POINTER and ALLOCATABLE dummies and array-valued results retain
their existing specialization limits. Recursive functions with array dummies remain unsupported.
Run `python tests/test_scalar_array_functions.py` and
`python tests/test_function_arrays.py`.

### Forwarded array-valued function calls

Runtime reduction results can be assigned directly to sections of allocatable
arrays and derived-type components. Section operands are captured before the
reduction, and destination-section lowering resumes once the result axes are
known. This covers DIM, runtime DIM, masks and scalar-result broadcasting; see
`python tests/test_section_reduction_assignment.py`.

Large fixed-shape numeric and LOGICAL section assignments with scalar or
array-expression right-hand sides use runtime loops rather than one generated
statement per element. Literal constructor assignments retain their existing
element expansion. Array right-hand sides are
buffered before destination writes to preserve overlapping and reversed sections;
scalar right-hand sides are evaluated once. See
`python tests/test_large_section_assignments.py`.

Calls with derived-type arguments are lowered inside section lower bounds,
upper bounds, and strides, including sections used in derived constructors.
`python tests/test_derived_section_bound_calls.py` checks these calls with
allocatable components, empty/reversed sections and repeated assignments.

Numeric and LOGICAL fixed-shape sections passed to specialized INTENT(IN)
subroutine dummies are copied by runtime loops rather than per-element source
expansion. This includes multidimensional columns, strided and reversed sections,
and empty sections, without the former 1024-element section limit. See
`python tests/test_large_section_arguments.py`.

Supported array-valued functions can be called from procedures with assumed-shape
arguments. Function expansion waits for procedure association to supply the
actual extents and CHARACTER lengths. This applies to named and defined-operator
calls, including `xfind .in. xdata`, and to numeric, LOGICAL and CHARACTER results.
Nested forwarding, keyword calls, empty arrays and supported constant CHARACTER
sections are covered by `python tests/test_forward_array_results.py`.

An array-valued function result can itself be an actual argument to another
procedure inside an assumed-shape wrapper. Function lowering completes before
the outer call associates its arguments, retaining the result's type, rank and
active shape. Nested functions, OPTIONAL variants, genuine PACK inputs, empty
results and repeated calls are covered by
`python tests/test_nested_array_function_arguments.py`, with interpreter/native
O0/O2 checks against gfortran. A function is evaluated once before its consumer.

Bounded function arguments and fixed-shape results support ranks 1 through 15,
independently of PURE; argument and result ranks may differ. This includes
numeric, LOGICAL and fixed-length CHARACTER arrays. Each dimension is preserved
until storage lowering, with Fortran column-major element order. Read-only
inputs are copied; explicit INTENT(OUT)/INTENT(INOUT) variables retain caller
storage and side effects. Whole arrays, rank-one unit-stride sections, RESHAPE
and array constructor arguments, and fixed-shape results have no element limit;
ELEMENTAL calls expanded per element, such as CHARACTER ones, and other array
expression arguments, such as `abs(x)`, still allow at most 1024 elements. Extents and CHARACTER lengths must be known after association.
Source array elements copied into the function must be
initialized. Rank-one fixed-length CHARACTER ALLOCATABLE results can use bounded
ALLOCATABLE output helpers or direct `ALLOCATE` statements with proven finite
bounds; their active length is tracked separately from storage capacity. This
supports filtered `.exclude.` results and OPTIONAL `vec` calls with zero through
ten consecutive arguments, including empty results, assignment, argument passing
and list-directed/formatted output. `tests/183_optional_character_array_result.f90`
tests the original `util.f90` routine and preserves its consecutive-argument
assumption.
`python tests/test_character_allocatable_results.py` checks these paths against
the interpreter, native code at `-O0`/`-O2`, and gfortran when available.
`python tests/test_multirank_array_functions.py` covers PURE and ordinary
functions, ranks through 15, differing result ranks, sections, empty results,
array expressions, writable arguments and module side effects. `tests/programs/xabove_diag.f90`
prints `4.0 7.0 8.0` with or without PURE.

### Defined operators with array operands

Defined dotted operators such as `.notin.` can select functions with array
operands. Functions with read-only INTEGER, REAL, DOUBLE PRECISION
or LOGICAL array dummies specialize for statically known actual extents, including
assumed-shape dummies, fixed sections, constructors, supported array expressions
and empty arrays. The same specialization works for direct function calls and
supported numeric/LOGICAL results of ranks 1 through 15 whose extents resolve at
the call site. Input type, rank, extent and initialization checks remain active.

Scalar numeric and LOGICAL results can use runtime-sized array operands through
the descriptor association described under
[Array arguments to functions](#array-arguments-to-functions). Array-result specialization still
copies bounded inputs and supports at most 1024 elements per dummy. Recursive
specialization and unrestricted array-result ABIs remain unsupported. `tests/programs/xnotin.f90` now prints `F T`. Run
`python tests/test_array_operators.py` for interpreter/native O0/O2 checks and
gfortran comparisons.

Overloaded operators backed by ELEMENTAL functions also accept arrays. Scalar
operands broadcast on either side; unary operators preserve the operand shape.
Array operands must have the same rank and conform in every dimension, including
runtime checks for allocated arrays whose total sizes happen to be equal.
Intrinsic-spelling operators and user-defined dot operators use the same dispatch.
Supported numeric, LOGICAL and CHARACTER results work with ordinary assignments,
output, nested calls and reductions such as `ANY` and `ALL`.

Derived-type inputs use supported scalar primitive components, including nested
scalar records supported by the existing record layout. Only needed components
are forwarded, so unrelated uninitialized components are not read. A completely
unused record input retains a representative component for shape checking.
Fixed arrays through rank 15, sections, vector subscripts, empty arrays and
runtime-sized vectors/matrices are tested. Direct elemental function calls use
the same argument/result shape handling, and impure operator calls retain their
per-element effects. Record array components, ALLOCATABLE/POINTER components,
and derived-valued elemental results retain their existing limitations.

Run `python tests/test_elemental_operators.py` and
`python tests/test_character_operators.py` for interpreter, native O0/O2 and
gfortran comparisons. `tests/programs/xacv_elemental_derived_comparison.f90`
now prints `T`. Derived-type dummy arguments to ELEMENTAL subroutines also
work for supported scalar records and constant rank-one record arrays;
`tests/programs/xacv_elemental_derived_out.f90` prints `9 30 0`. Derived-valued
ELEMENTAL function results also work for supported scalar records and fixed
rank-one array calls; `tests/programs/xacv_elemental_derived_result.f90` prints
`9 30 45`.

### Generic and elemental dispatch

`tests/205_util_families.f90` and `python tests/test_remaining_util.py` cover
runtime CHARACTER constructors with array-valued items, elemental generic
dispatch (including keywords and matching-rank precedence), allocatable output
arguments and external integer input. Materialized multidimensional expressions
retain their original rank and shape during argument association; tests cover
reallocation, empty CHARACTER RESHAPE results, PAD/ORDER and ranks through 15.

### ELEMENTAL functions

ELEMENTAL functions support explicitly declared numeric, LOGICAL, and CHARACTER
scalar results and input dummies. Supported scalar derived-type INTENT(IN)
dummies are expanded into their needed components for scalar or array calls.
Derived-type scalar results with scalar primitive components (including nested
scalar records) support scalar and constant rank-one array calls. These calls
are evaluated once into result storage through a hidden elemental subroutine;
self-assignment, nested calls, scalar broadcasting, OPTIONAL/named inputs,
constructors, constant sections, vector subscripts, empty arrays, early RETURN,
and default component initialization are covered. IMPURE calls retain one call
per element in array element order. Scalar reductions such as SUM, COUNT and
DOT_PRODUCT remain scalar when supplied as arguments.

Derived-valued elemental results currently share the subroutine expansion's
limitations: runtime-sized and multidimensional array calls, CHARACTER
components in array results, array components, and ALLOCATABLE/POINTER
components are outside this subset. An ELEMENTAL result itself cannot be
ALLOCATABLE or POINTER.
Run `python tests/test_elemental_derived_results.py` for interpreter/native
O0/O2 checks with gfortran comparisons.

CHARACTER results and CHARACTER input dummies
are specialized into local buffers for each call. Assumed lengths follow the
actual argument, and result lengths such as `len(a)+len(b)` resolve using those
lengths. Fixed-length input dummies may consume the leading part of a longer
actual. Assignments retain normal padding and truncation.

Scalar calls, rank-one array constructors/variables/constant sections, scalar
broadcasting, nested calls, named arguments, early RETURN, empty arrays, and
formatted PRINT/WRITE are supported for CHARACTER elemental functions.
Scalar actual expressions are evaluated once per array call; IMPURE element
calls run in array element order. LOGICAL results also work with the existing
numeric array expansion. Purity, INTENT(IN), shape compatibility, argument
kinds, and uninitialized reads remain checked.

CHARACTER lengths must be constant after specialization, and locals must be
explicitly declared. Runtime-length actuals
such as TRIM of a variable, higher-rank arrays in CHARACTER calls,
recursive CHARACTER calls, enclosing-procedure variable captures, internal
procedures inside these functions, and CHARACTER calls in WHERE remain
unsupported and are diagnosed. This uses the compiler's existing fixed-size
local CHARACTER storage, not a general CHARACTER function-result ABI.
Run `python tests/test_character_elemental.py` for interpreter/native O0/O2
checks with gfortran comparisons. The main suite includes
`tests/143_character_elemental.f90`, matching `xchar_plus_char.f90`.

Local declarations and dummy arguments in specialized calls keep their own
array shape, allocation and pointer metadata. A scalar local may share a name
with an unrelated caller matrix, including through nested elemental calls.
Assumed-shape associations retain their actual extents, and BLOCK scopes retain
inherited descriptors for names they do not redeclare. Run
`python tests/test_array_scope_metadata.py` for the scope-collision regression
and checks of scalar, fixed-array and pointer bindings.

Scalar actual expressions in specialized ELEMENTAL procedure calls retain known
ELEMENTAL function result types, including nested calls and arithmetic or logical
expressions. See `tests/programs/xelemental_function_actual.f90`.

### ELEMENTAL subroutines and purity

Scalar calls to ELEMENTAL subroutines support explicit INTENT(IN), INTENT(OUT),
and INTENT(INOUT), omitted OPTIONAL arguments, named arguments, and PRESENT tests.
Intrinsic purity classifications are shared through `src/intrinsics.rs`. PURE
procedures recognize rounding intrinsics, SIGN/DIM/DPROD, MERGE, array products,
lexical CHARACTER comparisons and ASSOCIATED, plus the existing intrinsic
families. RANDOM_NUMBER, RANDOM_SEED, CPU_TIME, SYSTEM_CLOCK and DATE_AND_TIME
remain impure. User-defined procedure declarations take precedence over intrinsic
names. Classification tests cover all registered intrinsic interfaces, while
`python tests/test_pure_intrinsics.py` checks supported execution and rejection
of impure calls. Some classified intrinsics still have incomplete semantic or
execution support; the purity catalog records their language classification.

ELEMENTAL implies PURE unless IMPURE is specified; PURE restrictions are checked.
IMPURE ELEMENTAL functions and subroutines may perform external I/O and other
supported side effects. Either prefix order (`impure elemental` or `elemental
impure`) is accepted. Supported dummy types
include INTEGER, REAL, DOUBLE PRECISION, LOGICAL, CHARACTER, and supported
scalar derived types. Derived dummies retain their INTENT and OPTIONAL
attributes when expanded into components. Scalar array-element outputs do not
read the previous component values, and INOUT updates are copied back even
when the procedure returns early. Assumed
CHARACTER dummy lengths are specialized to each scalar actual argument, with
normal assignment padding and truncation.

The four specific `set_optional_real`, `set_optional_integer`,
`set_optional_logical`, and `set_optional_character` routines from the utility
module are covered by the regression tests. Positional and named scalar calls
are supported, including through supported generic interfaces. Numeric and
LOGICAL array actuals with constant rank-one extents are also supported for
ELEMENTAL subroutines: constructors, supported array expressions, whole arrays,
and constant sections. Derived-type arrays with scalar primitive components,
including nested scalar records, use the same expansion. Scalar inputs are
broadcast; writable actuals must be
arrays or sections when any actual is an array. Calls run in array element order,
preserving impure I/O and side effects. Output sections need not be initialized.
Mismatched extents and calls to impure procedures from PURE or FORALL contexts
are rejected. Dynamic extents, multidimensional whole-array subroutine actuals,
CHARACTER array actuals, and the separate allocatable setters remain outside
this subset. Unsupported actual expressions and mismatched types produce
diagnostics. Explicit-length CHARACTER dummies currently require matching actual
lengths.

`python tests/test_elemental_derived_subroutines.py` checks derived OUT/INOUT,
OPTIONAL/PRESENT, keywords, array and section calls, nested records, and
scalar array-element outputs against gfortran and interpreter/native O0/O2.

`tests/126_impure_elemental.f90` and `python tests/test_impure_elemental.py`
check constructors, writable arrays and sections, OPTIONAL/named arguments,
impure elemental function calls, empty arrays, side-effect order, and diagnostics
through interpreter/native O0/O2 execution with gfortran comparisons.

### Dummy procedures and procedure pointers

Dummy procedures can be declared EXTERNAL (including `real(8), external :: f`),
in an interface block, with `procedure(iface)` naming an ABSTRACT interface or
another procedure, or simply by being invoked. Each call passing procedure names
is redirected at compile time to a copy of the callee with the dummies replaced
by those procedures, so calls through dummies are direct calls. Module,
internal and external procedures can be passed, a dummy procedure can be
forwarded to another procedure, and keyword actuals work. Procedure pointers
declared with `procedure(iface), pointer` become an integer tag: `p => f`,
`nullify(p)`, `associated(p)` and `associated(p, f)` update or test it, and calls
through `p` dispatch on it to the procedures ever associated with `p` in its
scope. Procedure pointer components, procedure pointers passed as actual
arguments or used as dummies, OPTIONAL dummy procedures and passing intrinsic
procedures are not yet supported. A module containing a procedure with dummy
procedures must be compiled with its callers, since the copies are made per
call. Run `python tests/test_procedure_arguments.py`.

### Internal procedures and host association

Functions, subroutines and the main program accept CONTAINS with internal
functions/subroutines. Internal procedures can read and update INTEGER(4/8),
REAL(4/8), and LOGICAL variables explicitly declared in the enclosing procedure
or main program, including its dummy arguments and array elements of ranks 1
through 15. Array accesses retain lower bounds,
check subscripts, and share the original storage and initialization flags.
An internal procedure of the main program that has array, CHARACTER or POINTER
dummy arguments, or is ELEMENTAL, is instead expanded at each call, so it also
sees the main program's other variables, such as ALLOCATABLE arrays.
SIZE, SHAPE, LBOUND, and UBOUND inquiries are supported; DIM must be constant.

Recursive helpers with scalar input arguments forward their host environment,
so a quick-sort helper can modify the enclosing procedure's arrays without
copying them. Sibling calls share that environment, local declarations shadow
host variables, and identical helper names in separate hosts remain distinct.
Reads of undefined host values retain runtime checks; writes to host INTENT(IN)
variables are rejected. The native backend passes borrowed storage addresses;
the interpreter uses handles identifying the owning call frame.

Capturing CHARACTER, POINTER, ALLOCATABLE, or OPTIONAL host objects, whole-array
host values/assignments, sections, READ/I/O definitions through host objects,
and forwarding captured objects directly to ordinary subroutine dummies or
internal writable dummy arguments remain unsupported. Internal function results
must have explicit types. Internal procedures cannot yet
contain further internal procedures. Procedure values/escaping closures are
unsupported; recursive array/output dummy arguments retain the limits below.
Run `python tests/test_internal_procedures.py` for interpreter/native O0/O2 checks
with gfortran comparisons. The Cargo suite includes
`tests/140_internal_procedures.f90`.

### Recursive procedures

RECURSIVE is accepted on functions and subroutines, including PURE RECURSIVE
and RECURSIVE PURE prefixes. Scalar recursive functions can reuse OPTIONAL
specializations. Ordinary INTEGER, REAL and LOGICAL array dummies can also use
native recursive subroutine calls with borrowed data, initialization masks and
per-dimension bounds/strides. Direct and mutual recursion, OPTIONAL presence
variants, ranks through 15, writable scalar dummies, explicit-shape bounds and
runtime-sized actual arrays are supported. Each activation has its own scalar
locals; supported initialized locals retain shared SAVE storage. Whole-array
forwarding preserves storage identity rather than converting it into a value
expression. Section actuals use the existing association/copy rules.

Array operations and sections inside these procedures use runtime descriptors.
Overlapping assignments use temporary heap storage when the extent is unknown.
Bounds and per-element initialization checks remain active, and INTENT(OUT)
invalidates previous values on every recursive entry. Explicit-shape checks use
captured scalar actuals even when the bound dummy follows the array in the
argument list. `tests/programs/xrecursive_optional_array.f90` reproduces the `util.f90` gen/combos
case; `tests/188_recursive_optional_array.f90` is its numbered regression.
Run `python tests/test_recursive_arrays.py` for interpreter/native O0/O2 and
gfortran comparisons.

Recursive CHARACTER, ALLOCATABLE and POINTER dummy interfaces and forwarding a
new section on a recursive edge retain their existing limitations. Recursive
functions with array dummies or array-valued results remain outside this subset.
Bounded ALLOCATABLE output recursion can still terminate during specialization, as in `tests/programs/xpure_recursive.f90`,
whose recursive call omits the optional bounds and selects the copy branch.

## Modules

### Module data and procedure association

Procedures can read module named constants and share mutable module variables.
USE association supports variables, constants, procedures and derived types,
including ONLY lists, renaming, reexports and generic interface extensions.
PUBLIC/PRIVATE access and local shadowing are checked. Procedures with the same
name in different modules retain distinct identities.

Named generic procedures, defined operators and defined assignment can merge
across USE-associated modules and local interface extensions. ONLY imports,
renamed generics/operators and public re-exports retain all accessible specific
procedures; importing the same specific through multiple paths does not duplicate
it. ASSIGNMENT(=) is accepted in ONLY and PUBLIC/PRIVATE lists. Duplicate argument
or operand signatures in the merged interface are diagnosed, including unused
operator and assignment interfaces. Regression coverage is in
`tests/test_interface_merging.py`.

Extending an intrinsic generic retains its built-in alternatives. For example,
a module may define `MAX` for a date type and still use `max(1,mod_year)`.
User-defined specifics are selected first; unmatched calls to known intrinsic
functions use the intrinsic path. Ambiguous user specifics remain errors.
The shared fallback checks type/kind/rank rules for MIN/MAX, reductions,
numeric conversions and REPEAT; other known functions retain their ordinary
intrinsic validation and backend limits. No argument conversion is introduced
to make a user-defined specific match.

Constant processing preserves user keywords until selection. Intrinsic keyword
order is used to infer nested result kinds/ranks, then the selected built-in
call is normalized for later lowering. Variadic MIN/MAX, reduction DIM/MASK,
and conversion KIND keywords are covered. Existing limits on materializing
some DIM-reduction results as procedure actuals remain unchanged.
Run `python tests/test_intrinsic_generics.py` and
`python tests/test_repeat_generic.py`; the four `tests/programs/xacv_*_intrinsic.f90`
examples exercise both built-in and derived-type calls with gfortran comparisons.

INTEGER, REAL, LOGICAL and fixed-length CHARACTER data use shared storage across
calls, including supported fixed arrays and runtime-allocated numeric arrays.
Allocation status, shape and initialization checks persist across procedures.
PURE procedures can read module data but cannot modify nonlocal variables.
Existing type and rank limits still apply. Numeric module arrays use heap storage;
CHARACTER allocation retains its existing bounds. This does not add a general
module pointer or derived-type storage ABI.

Supply module sources together, for example
`rfort m1.f90 m2.f90 main.f90 --run`, or compile them separately using `-c`
and the persistent `.rmod` interfaces described in [usage.md](usage.md). Run
`python tests/test_module_scope.py` for interpreter, native O0/O2 and gfortran
comparisons and `python tests/test_module_compilation.py` for cross-object tests.

### Intrinsic modules

ISO_FORTRAN_ENV provides the INTEGER kind constants INT8/INT16/INT32/
INT64 (1/2/4/8) and REAL32/REAL64 (4/8). REAL128 is -1 to indicate an unsupported
real kind. It also provides INPUT_UNIT, OUTPUT_UNIT and ERROR_UNIT (5, 6 and 0),
IOSTAT_END and IOSTAT_EOR (-1 and -2), the storage sizes, the arrays
INTEGER_KINDS, REAL_KINDS, LOGICAL_KINDS and CHARACTER_KINDS, and the functions
COMPILER_VERSION and COMPILER_OPTIONS. WRITE to ERROR_UNIT is not yet
supported. The intrinsics IS_IOSTAT_END and IS_IOSTAT_EOR test an IOSTAT value. USE, INTRINSIC :: ISO_FORTRAN_ENV, ordinary USE and USE :: forms support
ONLY lists, host association and re-export through user modules. Explicit
INTRINSIC selects the built-in module even if a user module has the same name;
NON_INTRINSIC requires a user module in the source group or a saved `.rmod` interface. Other ISO_FORTRAN_ENV entities are not yet implemented.
Run `python tests/test_intrinsic_env.py`.

IEEE_ARITHMETIC (Fortran 2003) supports REAL kinds 4 and 8 through the elemental
functions IEEE_VALUE, IEEE_IS_NAN, IEEE_IS_FINITE, IEEE_IS_NORMAL and IEEE_UNORDERED.
IEEE_CLASS_TYPE and the named class constants for signaling/quiet NaNs, signed
infinities, normal/subnormal values and signed zeros are available; DENORMAL and
SUBNORMAL spellings denote the same classes. IEEE_VALUE constructs representative
bit patterns without division by zero or invalid arithmetic. Its X argument
supplies the result type, kind and elemental shape without reading the stored
value; an undefined scalar or allocated array is a valid mold. CLASS still
requires a defined value, and using X as a mold does not initialize it. Fixed
and runtime-sized arrays retain shape/conformance checks. See
`tests/programs/xieee_value_mold.f90` and `tests/test_ieee_value_mold.py`.
Signaling NaNs may be quieted by the compiler's intermediate real-kind
conversions.
Scalar and array arguments, keyword arguments, ONLY/renaming, host association,
module re-export and calls from PURE procedures use the ordinary module machinery.
Both ordinary USE and USE, INTRINSIC are supported.

This is a partial implementation: IEEE_CLASS, class comparisons, support inquiries,
rounding modes, exception flags/control and the other IEEE procedures are not yet
provided. Class-component privacy is not yet enforced. These functions are not yet
supported in constant initialization expressions. An existing unrelated limitation
in WHERE assignment to allocatable arrays can overwrite unselected elements;
use an explicit element loop for that case.
Run `python tests/test_ieee_arithmetic.py`.

## Input and output

### Formatted output

A FORMAT statement, as in `print 10, x` with `10 format (2e16.3)`, gives its
format to PRINT, WRITE and READ that name its label, including `fmt=10` and
`read 10, x`. Labels are local to each program unit.

WRITE and PRINT accept scalar CHARACTER format expressions: literals, named
PARAMETERs, variables (including formats read from input), concatenation, and
TRIM. WRITE supports standard output (`*`, constant unit 6, or an INTEGER
unit variable containing 6), with or without
an output list. Runtime formats are checked for syntax and item types before
the statement prints. Uninitialized format variables use the usual
initialization checks.

The edit descriptors are A, I, B, O, Z (with `w.m`), F, E, ES, EN, D and G
(with `w.d`, and `Ee` exponent widths for E, ES, EN and G), G0 and G0.d, L,
quoted literals, `nX`, T, TL, TR, `/`, `:`, the scale factor `kP`, sign control
SP, SS and S, and BN and BZ (which do not affect output). Groups may be nested,
and `*(...)` repeats until the output list ends. When the format ends with
items left, a new record starts and output continues from the last top-level
group, or from the start of the format if it has none (format reversion).
G0 writes a REAL value with 9 significant digits for kind 4 and 17 for kind 8,
as gfortran does. Numbers are rounded to nearest, ties to even, from their
exact binary value. Output matches gfortran for these descriptors, including
Infinity and NaN, asterisks for fields that do not fit, and dropping the
optional leading zero (`.500`) when the field is too narrow or for F0.d. DC
and the rounding-mode descriptors (RU, RD, RZ, RN, RC, RP) are not supported.
The interpreter does not track INTEGER kinds, so B, O and Z of a negative
value of a non-default kind use the default kind's bits there.
Run `python tests/test_formatted_output.py`.

WRITE controls may be positional or named: `WRITE(*,FMT=fmt)` and
`WRITE(UNIT=6,FMT=fmt)` use the same format path. Named UNIT, FMT, and ADVANCE
controls may be reordered; positional controls must precede named controls.
Duplicate controls and missing UNIT/FMT are rejected. This also covers empty
output lists, list-directed output implied-DOs, and the supported scratch-file
output subset.
Run `python tests/test_write_controls.py` for interpreter/native O0/O2 checks
with gfortran comparisons; `tests/138_write_controls.f90` is in the Cargo suite.

Formatted WRITE accepts `ADVANCE='no'` to continue on the same output line and
`ADVANCE='yes'` for the default behavior. ADVANCE also accepts scalar CHARACTER
variables and supported expressions, including specialized function results.
Its value is evaluated once and checked for case-insensitive `yes` or `no`,
allowing trailing blanks. Nonadvancing
writes flush stdout, so prompts appear before a following READ. A final
nonadvancing write leaves stdout without an added newline, including at STOP.
ADVANCE cannot be used with list-directed (`*`) formats.

External formatted WRITE also accepts ADVANCE with literal or runtime values.
Successive nonadvancing writes append fragments to the current record; an
advancing WRITE (including an empty list-directed WRITE) finishes it. Literal
and runtime formats, numeric/character items, and implied-DO output use the same
formatter as stdout. Terminate the final record with an advancing WRITE;
cross-statement positioning with T/TL edit descriptors is not tracked.
Run `python tests/test_file_nonadvancing.py` for CSV record assembly and readback,
runtime controls, empty fragments and slash descriptors.

Formatted internal WRITE to a scalar CHARACTER buffer supports literal or runtime
format strings, INTEGER/REAL/LOGICAL/CHARACTER items, arrays and implied-DO lists.
The same parser and descriptors are used for standard output. Each statement
captures its inputs before replacing the destination, pads unused characters
with blanks, and reports record overflow. PURE and ELEMENTAL procedures may
write local CHARACTER variables and writable dummy arguments; INTENT(IN),
nonlocal destinations and external I/O remain protected. This subset supports
one record. The internal file may also be an element of a CHARACTER array, such
as `write(names(i),'(i8)') i`, including multidimensional arrays and allocatable
components such as `write(self%columns(i),"('C',i0)") i`. Element subscripts are
captured before formatting, and the completed record is copied back through
checked array assignment. See `tests/programs/xinternal_write_element.f90`.
Whole arrays as internal files, IOSTAT and other
WRITE control options remain unsupported. List-directed internal WRITE (`write(s,*) n`) writes each
item as G0 after a blank.
`tests/programs/xstr_real.f90`, `tests/172_internal_write.f90`, and
`python tests/test_internal_write.py` cover this behavior.
Captured array items also retain every element after vector selection and
function specialization. `tests/programs/xnow_format.f90` reproduces a calendar string written
from an integer vector selection; `python tests/test_util_regressions.py` checks
this alongside multidimensional, empty and runtime-sized array items.

Formatted PRINT/WRITE to standard output support nested implied-DO lists with
runtime INTEGER bounds and positive/negative steps. Controls are evaluated once
on entry to each implied-DO. Scalar SIZE/LBOUND/UBOUND inquiries retain their
integer type after allocation checks are inserted, including output that mixes
runtime-allocated local arrays with assumed-shape arguments. Allocation guards
remain active, and invalid formatted implied-DO controls report the PRINT/WRITE
source line. Transfer values, including CHARACTER substrings,
are copied into a statement-local list before formatting. Memory use scales
with the list's item count and CHARACTER lengths.
Finite nested format groups and a final unlimited-repeat group such as
`(*(1x,a))` are supported. Unlimited groups require a data descriptor and repeat
until the output list ends; finite formats retain the existing exact item-count
requirement. Runtime formats can also consume numeric and LOGICAL array items
and sections using their active element counts. Empty CHARACTER array items
inside zero-iteration implied-DOs are not read. Run
`python tests/test_output_units.py` and `python tests/test_formatted_implied_do.py` for interpreter/native
O0/O2 checks with gfortran comparisons. The Cargo suite includes
`tests/139_formatted_implied_do.f90`.

Transfer items inside implied-DOs must resolve to supported scalar values;
existing CHARACTER procedure-argument limitations remain. Numeric format labels,
format reversion remain unsupported outside the existing `(A)` file subset.
Formatted array items and implied-DO lists can also write sequential file units.
Format nesting is limited to 32 levels;
the existing limits of
256 for field width/precision and 10,000 for repeat counts apply.
The native formatting runtime is written in Rust, built with the compiler,
and embedded in `rfort`; no separate runtime installation is needed. Use
`rfort link` when linking emitted objects so it supplies this runtime.

### NAMELIST

NAMELIST declarations and external `READ(unit,nml=group)` /
`WRITE(unit,nml=group)` support intrinsic INTEGER, REAL, LOGICAL, COMPLEX and
fixed-length CHARACTER scalars and fixed-size arrays. A visible group may also
occupy the positional format slot, as in `write(unit,group)`. Use explicit
`NML=group` when supplying status or message controls. Repeated NAMELIST
statements extend a group. Groups declared in a host scope can be used by its
internal procedures. `UNIT`, `NML`, `IOSTAT` and `IOMSG` keywords can be reordered.

```fortran
integer :: n, unit, status
real :: values(3)
character(100) :: message
namelist /settings/ n, values
n = 3
values = [1.0, 2.0, 3.0]
open(newunit=unit, status="scratch")
write(unit, nml=settings)
rewind(unit)
read(unit, nml=settings, iostat=status, iomsg=message)
close(unit)
```

Input recognizes case-insensitive group/member names, `!` comments, quoted
strings with doubled quotes, multiline values, repetition counts and null
values. Omitted members and null array entries retain their previous values
and initialization state. Array values follow Fortran element order; input
such as `values(2)=4.0` assigns the selected element. Element designators
support ranks through 15, with rank and bounds checks. Ordinary CHARACTER
scalar variables can also serve as internal input files. Unknown names, bad
conversions, invalid designators, array overruns and disconnected units report
positive IOSTAT values; end of file reports a negative value. IOMSG is defined
on an error and left unchanged on success. An error without IOSTAT stops
execution. These behaviors are shared by native execution and the interpreter.

Within the input limits below, output uses quoted CHARACTER values and enough
REAL digits for round trips;
its whitespace and record layout need not match another compiler's output.
Tests read rfort output with gfortran and gfortran output with rfort.

This is partial NAMELIST support. Members currently require explicit intrinsic
declarations. Internal output, array sections/CHARACTER
substrings in input designators, ALLOCATABLE/POINTER members, derived-type
objects and USE association of group names remain unsupported. Controls other
than UNIT/NML/IOSTAT/IOMSG (including DELIM, ERR and END) are rejected. The
scanner currently accepts non-NUL ASCII input and limits each scalar token to
4096 bytes, reporting an error rather than truncating it. CHARACTER output
currently requires a literal declared length no greater than 4000.

Run `python tests/test_namelist.py` after building the compiler. The example
`xnamelist.f90` demonstrates a scratch-file round trip.

### List-directed output

List-directed PRINT/WRITE implied-DOs evaluate CHARACTER array elements,
substrings, concatenations and supported CHARACTER function calls inside each
iteration, including allocatable CHARACTER storage. Nested loops, runtime
bounds, reverse steps and empty loops are supported. Loop controls are evaluated
once on entry; zero-iteration loops do not evaluate their items. A variable
external unit is captured before evaluating output items. These rules also apply
to the supported formatted and internal WRITE paths. Run
`python tests/test_character_output_implied_do.py` for interpreter/native O0/O2
checks with gfortran comparisons.

List-directed LOGICAL output uses `T` and `F` for scalar values, array elements,
expressions and procedure results. CHARACTER strings containing `.TRUE.` or
`.FALSE.` retain their spelling. Run `python tests/test_logical_list_output.py`
for strict spelling checks in the interpreter and native code at O0/O2.
List-directed REAL output has 6 significant digits for REAL and 15 for DOUBLE
PRECISION, in fixed or exponent form as C's `%g` chooses, without trailing zeros
but with a digit after the decimal point, and with an unpadded exponent:
`0.5`, `60.0`, `1.0e-6`, `2.5e10`, `-0.0`, `Infinity`. An exactly integral value
is written as `n.0`. The interpreter and compiled code write the same text on
every platform. Run `python tests/test_goto_and_real_output.py`.
List-directed `PRINT *` with no output items emits a blank record. Large real
output, including `HUGE(0.0)` and `HUGE(0.0_8)`, no longer traps while choosing
an output representation. Values near the largest DOUBLE PRECISION number retain
enough output digits to avoid rounding beyond the finite range.
Run `python tests/test_numeric_model.py` for interpreter/native O0/O2 comparisons
with gfortran, statement boundaries, initialization behavior and invalid arguments.

### File and standard input

FLUSH writes all buffered output, and ENDFILE ends a file at its current
position. With `iostat=`, an OPEN that fails sets a nonzero status, sets IOMSG to
a message naming the file, and leaves NEWUNIT unchanged; CLOSE, REWIND,
BACKSPACE, ENDFILE and FLUSH accept IOSTAT, IOMSG and ERR, but a failure of one
of them stops the program, so IOSTAT is then always zero. READ accepts IOMSG
with IOSTAT; IOMSG is not set.

A formatted READ with a constant format of edit descriptors reads each record
whole, then gives each item its field: A, I, B, O, Z, F, E, ES, EN, D, G and L fields, X,
T, TL, TR and `/`, with repeat counts, groups and reversion. Items may be
scalars, array elements, fixed-size whole arrays and sections, and nested
implied-DO lists with constant bounds and steps. Sections use column-major
transfer order and can have descending or empty ranges. Implied-DO indices
are updated in input-list order, including their final and zero-trip values;
fixed-array SIZE/LBOUND/UBOUND inquiries are supported in the bounds. A blank numeric
field reads as zero, and an F, E, D or G field without a decimal point or
exponent has an implied one (G editing of INTEGER objects ignores the precision).
B/O/Z input accepts unsigned bit patterns for INTEGER kinds 1, 2, 4 and 8,
checks overflow, and supports BN/BZ blank editing. Formatted COMPLEX input
transfers the real and imaginary components with consecutive real descriptors;
whole arrays and bounded implied-DOs are supported. COMPLEX input sections and
subscripts containing function calls or array references remain unsupported.
Formatted input with runtime-sized arrays, runtime section/loop bounds or
runtime numeric formats is not yet supported. Bounded input expansion is limited
to 10000 entries (including loop-index updates). Records are
read up to 4096 characters. Formatted WRITE with an implied-DO list goes to a
file as well as to standard output.

Unformatted stream files are opened with `access='stream', form='unformatted'`.
A READ or WRITE without a format transfers each value as the bytes of its kind,
with no record markers, as gfortran does: INTEGER kinds 1, 2, 4 and 8, REAL and
DOUBLE PRECISION, LOGICAL (4 bytes) and CHARACTER (its length in bytes). Items may
be scalars, expressions such as `size(a)` and `shape(a)`, whole arrays and
sections (fixed or runtime-sized, including runtime CHARACTER arrays) in array
element order, implied-DO lists, and derived-type objects and arrays,
component by component. READ accepts `pos=` (the byte position, counted from 1),
IOSTAT (negative at end of file) and IOMSG; without IOSTAT, an end of file or
error stops the program. REWIND repositions the file. COMPLEX values, ERR=,
END= and unformatted sequential or direct access are not yet supported.

Sequential formatted files support `OPEN(unit=20,file='data.txt',status='old',action='read')`,
`READ(20,'(a)') text`, `BACKSPACE(20)`, `REWIND(20)`, and `CLOSE(20)`.
FILE may be a runtime scalar CHARACTER expression. Named files also support
NEWUNIT, STATUS='replace' with ACTION='write' or 'readwrite', and
STATUS='old' with ACTION='read', 'write' or 'readwrite'. The default
STATUS='unknown', ACTION='readwrite' combination opens or creates a file;
STATUS='unknown' also accepts ACTION='write' or 'read', and STATUS='new'
accepts ACTION='write' or 'readwrite' (without checking that the file is
absent). CLOSE accepts STATUS='keep' or 'delete'.
OPEN accepts constant POSITION='asis', 'rewind' or 'append'. For a new
connection, ASIS and REWIND start at the beginning; APPEND positions at the end
after opening or creating the file. APPEND does not force later writes to the
end: REWIND, BACKSPACE and stream positioning retain their usual behavior.
POSITION='append' also works with ACTION='read' and unformatted stream files.
STATUS='replace' still discards existing contents before positioning; an empty
scratch file starts at the same position for all three values. Reopening an
already connected unit remains unsupported. Run `python tests/test_open_position.py`
for interpreter/native O0/O2 comparisons with gfortran.
Formatted and list-directed WRITE go to a
connected unit; list-directed file output writes a blank, then each item as G0
separated by blanks, which list-directed READ reads back. Unit 0 writes to
standard error unless a file is connected to it. Explicit units are
0 through 1023 except preconnected units 5 and 6. `(A)` reads accept one scalar
CHARACTER destination; input records must contain non-NUL ASCII. Reads consume
the entire record and handle LF/CRLF endings.
`READ(unit,'(a)',iostat=status) text` reports zero on success, a negative value
at end of file, and a positive value on an input, encoding, or connection error.
IOSTAT must be a writable scalar INTEGER of kind 4 or 8. Failed reads leave the
CHARACTER destination undefined; reading it then triggers an initialization error.
The reader consumes invalid records so a subsequent read can continue with the
next record. These reads accept standard input as `*`, constant unit 5, or an
INTEGER unit variable whose value is 5. Input records may end without a final newline.

List-directed READ uses a shared streaming scanner in native and interpreted
execution. There is no fixed record-length limit: storage grows with token
length, rather than record length. A statement can read multiple INTEGER
(kind 1, 2, 4 or 8), REAL (kind 4 or 8), LOGICAL, CHARACTER and COMPLEX
(kind 4 or 8) values, continuing across records as necessary. INTEGER range
checks, E/D exponents, omitted exponent letters, repetition, null fields,
slash termination, quoted strings, doubled quotes and quoted continuation
across external records are supported. Null/slash fields preserve the old
value and its initialization state. The remainder of the final record is
consumed before the next advancing READ.

Supported destinations include numeric/LOGICAL arrays, runtime-sized arrays,
multidimensional triplet sections, bounded CHARACTER arrays and scalar
COMPLEX or whole COMPLEX arrays. Nested input implied-DO lists accept runtime
bounds and positive or negative steps. IOSTAT accepts INTEGER kinds 1, 2, 4
and 8. Standard input (`*` or unit 5), connected external units and scalar
CHARACTER internal files use the same scanner. Formatted READ with a constant
format also reads a scalar CHARACTER internal file as one record; a READ past
it reaches end of file. CHARACTER array elements can be internal files for
both forms of READ, such as `read(s(i),'(i8)') k`. Run
`python tests/test_list_read.py` for interpreter/native O0/O2 checks against
gfortran, including `tests/io/write_read.f90`, a copy of `xwrite_read.f90`.

Default derived-type input expands scalar records into writable components in
declaration order, including nested records and fixed-size component arrays.
List-directed READ supports these records on standard input, connected files,
and scalar CHARACTER internal files; constant-format READ supports them too.
Records can be mixed with primitive items and runtime list-directed implied-DOs
in a single input session, retaining null fields, repetition counts, slash
termination and IOSTAT handling. Records with ALLOCATABLE or POINTER components
require defined I/O and are rejected. Run `python tests/test_derived_read.py`.

This is not yet complete Fortran input support: CHARACTER internal-file arrays,
COMPLEX section destinations, vector subscripts, whole derived-array destinations
in formatted/list-directed READ and defined I/O,
DECIMAL='COMMA', IOMSG and combined status controls remain unsupported.

The format may also be a runtime scalar CHARACTER expression, including a
variable, `TRIM`, concatenation, or substring. `READ(unit,fmt,iostat=status)`
validates the format before consuming input; the supported format is `(A)`
(case-insensitive, with optional whitespace). An invalid or unsupported runtime
format gives a positive IOSTAT and leaves the destination undefined. Without
IOSTAT it stops with a runtime error. Numeric input descriptors and CHARACTER
field widths remain unsupported. Run `python tests/test_runtime_read_format.py`
for interpreter/native O0/O2 checks, including optional format dummies and
CHARACTER array-element destinations.

Empty list-directed `READ(unit,*)` and `READ(*,*)` consume exactly one record
without interpreting its contents. They support connected sequential formatted
files, scratch files, and standard input, including blank/long records and a
final record without a newline. `IOSTAT=` accepts a writable scalar INTEGER of
kind 4 or 8 and reports zero on success, a negative value at EOF, and a positive
value on an input or connection error. Without IOSTAT, failures stop with a
runtime error. Continuations, semicolon separators, and single-line IF statements
are supported. Run `python tests/test_empty_read.py` for interpreter/native
O0/O2 checks with gfortran comparisons.

Empty formatted reads such as `READ(unit,'(A)')` also work, including FORMAT
labels and `IOSTAT=`/`END=` handling. Constant formats use the existing format
parser: leading slash descriptors advance additional records and a colon
terminates format processing when the input list is empty. No field is parsed
or converted. Runtime CHARACTER format strings support `(A)`, with invalid
formats diagnosed through the existing record-reader checks. Constant-format
empty reads of scalar internal CHARACTER files leave their contents unchanged.

READ accepts named `UNIT=` and `FMT=` controls, including reordered keywords.
`END=label` on a list-directed or scalar CHARACTER formatted READ branches
at EOF to a following labelled statement in the same or an enclosing statement
sequence. It can exit nested loops; other input errors still stop execution.
Backward branches, transfers into loops or IF arms, and combining END with
IOSTAT remain unsupported. Use named EXIT/CYCLE for transfers across the
region skipped by END; unqualified transfers there are diagnosed.
Run `python tests/test_read_end.py` for interpreter/native O0/O2 checks
with gfortran comparisons.

Scratch files support `OPEN(newunit=iu,status='scratch',action='readwrite')`
or an explicit UNIT, with sequential formatted access. NEWUNIT assigns a
negative unit and supports up to 1024 simultaneous scratch connections.
`WRITE(iu,'(a)')` accepts scalar CHARACTER items, producing a record for each
item through format reversion; an empty list writes an empty record. Writes
replace subsequent records at the current position. Scratch files are deleted
on close. `INQUIRE` accepts exactly one of `UNIT=` or `FILE=`, with any
combination of scalar-variable outputs `EXIST=`, `OPENED=`, `NAMED=`, `NUMBER=`,
`NAME=`, `ACTION=`, `IOSTAT=` and `IOMSG=`, plus an `ERR=` statement label.
Controls may be reordered; UNIT may also be positional. Unit queries recognize
preconnected units 0, 5 and 6 and scratch NEWUNIT connections. `EXIST` tests
filesystem existence for FILE queries independently of connection state: a
closed file can exist with OPENED false. A missing file is a successful inquiry.
Unconnected queries return OPENED false, NUMBER -1 and IOSTAT zero. Unit existence
reflects the supported unit range and active NEWUNIT connections.
`NAMED` distinguishes named connections from scratch connections. FILE queries
return the supplied filename in NAME, padded or truncated to the destination
length. NAME is undefined by Fortran for an unnamed or unconnected unit; rfort
fills it with blanks. `ACTION` reports READ, WRITE or READWRITE from the connection,
and UNDEFINED when unconnected. Filenames ignore trailing blanks and connection
lookup compares normalized absolute paths; distinct hard-link names are not equated.
Invalid queries set a nonzero IOSTAT and, if supplied, an explanatory IOMSG.
IOMSG is unchanged on success. ERR transfers after status/message assignment;
without IOSTAT or ERR an inquiry error terminates execution. Forward and backward
ERR labels use the existing statement-label machinery. Run
`python tests/test_inquire.py` for interpreter/native O0/O2 checks against gfortran,
including connection changes, scratch files, access modes and error branches.
Other inquiry properties, including SIZE and the separate IOLENGTH form, remain
unsupported.
`(A)` READ destinations may also be dynamically indexed local CHARACTER elements.

Formatted external WRITE controls other than ADVANCE remain unsupported.
Connections are shared within a
compiled source group. Native file I/O currently requires a 64-bit target.
Run `python tests/test_read_comments.py` for interpreter/native O0/O2 and
gfortran comparisons of these features.

Internal list-directed READ uses the shared scanner described above, including
mixed-type input lists and bounded CHARACTER-array destinations. Scalar
CHARACTER destinations use blank padding or truncation. Failed scalar transfers
leave their destination undefined; null fields preserve its definition status.

## System, random and timing intrinsics

### Command line and environment

`GET_COMMAND_ARGUMENT(NUMBER, VALUE, LENGTH, STATUS)` supports keyword
arguments and optional outputs. NUMBER, LENGTH and STATUS use default INTEGER.
VALUE is blank-padded or truncated; LENGTH reports the complete argument length.
STATUS is 0 for success, -1 for truncation, and positive for a missing argument.
Argument zero is the native executable name, or the source filename when
interpreting. Scalar array-element outputs and assumed-length CHARACTER dummies
are supported, including zero-length VALUE buffers. The intrinsic is impure.
Pass program arguments after `--`: `rfort main.f90 --run -- "hello world"` or
`rfort run main.f90 -- "hello world"`. Arguments after `--` retain spaces, empty
strings and leading hyphens. Run `python tests/test_command_argument.py` for
interpreter/native O0/O2 and gfortran comparisons.

The GNU `GETCWD` subroutine is supported as `CALL GETCWD(C, STATUS)` with
optional STATUS, including keyword arguments. C is a writable scalar CHARACTER
variable or array element; STATUS is INTEGER kind 4 or 8. Success returns 0 and
blank-pads C. On failure C is blank and STATUS receives a nonzero host error
code. The function form is not implemented. Run `python tests/test_getcwd.py`
for interpreter/native O0/O2 and gfortran comparisons. Initialization errors
now include the source statement location, including calls in combined files.

`GET_ENVIRONMENT_VARIABLE(NAME, VALUE, LENGTH, STATUS, TRIM_NAME)` supports
positional and keyword arguments. NAME accepts scalar CHARACTER text and
TRIM_NAME defaults to true. VALUE is blank-padded or truncated; LENGTH reports
the full value length. STATUS is 0 for success, -1 for truncation, and 1 for a
missing variable. LENGTH and STATUS require default INTEGER outputs. The
intrinsic is impure. It reads the host C runtime environment on Windows, Linux
and macOS; Windows treats an empty environment assignment as removal. Run
`python tests/test_environment.py` for interpreter/native O0/O2 and gfortran checks.

### Intrinsic keywords and RANDOM_SEED

Intrinsic keyword interfaces are centralized in `src/named_arguments.rs`, with
coverage checks for the intrinsic catalog. Added interfaces cover scalar
conversions, CHARACTER inquiries, numeric-model/kind inquiries, array operations,
and variadic `MIN`/`MAX`, including reordered keywords and omitted optional
arguments. `KIND` uses its standard argument name `X`.
`RANDOM_SEED(SIZE=...)`, `RANDOM_SEED(PUT=...)` and `RANDOM_SEED(GET=...)` now work
in the interpreter and native code. The seed representation uses two default
INTEGER words for the existing 64-bit generator state. SIZE defines a scalar
or an array element; PUT/GET require a rank-one default INTEGER array of at least
two elements. PUT is an input argument and accepts compatible array expressions,
including arithmetic, constructors, sections and PACK; GET requires writable
storage. Expressions use the same array materialization as other procedure
inputs. `tests/programs/xrandom_seed_expression.f90` compares expression and variable inputs.
GET can save the current stream for restoration with PUT.
The no-argument form retains its deterministic reset. `tests/programs/xrandom_seed.f90` checks
stream restoration and output range; `tests/test_intrinsic_keywords.py` checks
keyword association and seed argument/intent diagnostics.

### CPU_TIME, SYSTEM_CLOCK and DATE_AND_TIME

`CPU_TIME(TIME=...)` and `SYSTEM_CLOCK(COUNT=..., COUNT_RATE=..., COUNT_MAX=...)`
work in the interpreter and native code, including reordered keywords, omitted
clock outputs, scalar array elements and output dummy arguments. CPU_TIME returns
process CPU seconds in REAL kind 4 or 8; SYSTEM_CLOCK uses a monotonic clock.
INTEGER kind 4 counts use 1,000 ticks per second; kind 8 counts use 1,000,000,000.
COUNT_MAX is the largest positive integer of the selected kind, and counts wrap
to zero. Integer outputs must share a kind; COUNT_RATE can also be REAL. If the
OS clock is unavailable, CPU_TIME returns -1, and SYSTEM_CLOCK returns
-HUGE(COUNT) with zero rate and maximum. Both intrinsics are rejected in PURE
procedures. The OS adapters cover Windows, Linux and macOS; this change was tested
on Windows. `tests/programs/xtiming.f90` demonstrates both calls, and `tests/test_timing.py`
checks the interpreter, native code at O0/O2, and gfortran when available.
`DATE_AND_TIME` supports all four optional keywords (`DATE`, `TIME`, `ZONE`,
`VALUES`) in the interpreter and native code. One snapshot produces local date
`yyyymmdd`, time `hhmmss.sss`, UTC offset `+hhmm`/`-hhmm` and the eight integer
fields. CHARACTER outputs are truncated or padded to their declared lengths,
and scalar elements of supported CHARACTER arrays are accepted. VALUES accepts
a writable whole rank-one INTEGER array of kind 2, 4 or 8 with at least eight elements;
non-default lower bounds work, and only the first eight elements are written.
Unavailable clocks produce blank strings and negative HUGE values. PURE calls,
read-only outputs and invalid types/ranks are diagnosed. Windows, Linux and macOS
have OS adapters; validation here ran on Windows. `tests/programs/xdate_and_time.f90` is a small
example, and `tests/test_date_and_time.py` checks consistency and argument rules
against the interpreter, native O0/O2 and gfortran when available.

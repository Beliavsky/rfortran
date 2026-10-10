Progress toward compiling `kind.f90 util.f90 xutil.f90`

The first completed checkpoint fixes character-literal lexing and semicolon
statement separators. `util.f90` uses strings containing a single backslash
when building Windows paths, and places several statements on one line.
Previously, the lexer interpreted a backslash before a quote as a C escape,
swallowing subsequent source text and producing misleading diagnostics.

Backslashes are now literal characters. Quotes inside a string are represented
by doubled delimiters: `'It''s Fortran'` or `"Say ""hello"""`. Semicolons outside
strings and comments separate statements. Unterminated strings stop at the
line boundary instead of consuming subsequent lines. Dotted defined-operator
names such as `.in.` are now recognized as individual tokens, including in
OPERATOR interface/accessibility declarations; their expression syntax and
execution remain unimplemented. Continued character
literals across source lines are not implemented by this change.

Test 59 is a small executable reproduction covering these cases, including
empty strings, Windows paths, and semicolons after CASE selectors. Its output
is checked against gfortran and both compiler backends. Lexer unit tests also
check token positions and recovery after an unterminated literal.

The old block test used the nonstandard C-style `\n` escape. It now uses two
PRINT statements to retain its intended output under Fortran string rules.
Code that relied on C escapes in source strings must similarly be updated;
there is currently no switch to enable backslash escapes.

The second checkpoint adds nonempty `USE module, ONLY: name, ...` lists,
including `OPERATOR(...)` imports. Selected constants and kind parameters
propagate through module dependency chains; procedures, derived types, and
operators are checked for accessibility before lowering. Missing/private
imports, excluded module entities, conflicting local declarations, and
ambiguous imports produce errors. Public constants can be re-exported.
Tests 60 and 61 cover module chains, private helpers, local names, selected
types, and selected operators. The batch tests split test 60 into three files.

This remains whole-source-group module support: put dependencies before their
users. Renaming with `=>` and empty ONLY lists are not supported yet. Backends
still require unique module procedure/type names within a compilation;
this change does not implement module symbol mangling or `.mod` loading.

The third checkpoint makes `default` a contextual keyword in `CASE DEFAULT`.
It can otherwise name a variable, procedure, or derived-type component, and
appear in PUBLIC and USE ONLY lists. Test 62 covers those uses alongside
ordinary and continued CASE DEFAULT selectors, with output checked against
gfortran, native code at O0/O2, and the interpreter.

The fourth checkpoint adds named generic interfaces listing MODULE PROCEDUREs.
Calls select a specific function or subroutine using positional argument count,
type, kind, and rank, without inserting numeric conversions to find a match.
Generic names can be public while their implementations remain private;
USE ONLY, re-exporting, and extension/merging through USE are supported.
Unmatched calls, ambiguous overloads, mixed function/subroutine interfaces,
missing procedures, and mismatched END INTERFACE names are diagnosed.
END INTERFACE can repeat a generic name or OPERATOR specification.

Tests 63 and 64 exercise nested calls, numeric kinds, scalar/array subroutines,
derived/character/logical arguments, and module association. Their output is
checked against gfortran, native code at O0/O2, and the interpreter. The batch
suite also compiles test 63 as a three-file dependency chain.
This is not full generic-interface support: explicit interface bodies,
keyword actual arguments, optional-argument overload selection, extension of
intrinsic generics, and generic constructors are not implemented. Existing array/backend limits still apply,
and specific procedure names must remain unique within a compilation.

The fifth checkpoint adds named INTEGER kind selectors: `integer(kind=k)`
and `integer(k)`, where `k` is an accessible integer PARAMETER. Numeric
`integer(kind=8)` is also accepted. Selectors resolve before overload selection
and derived-type lowering, including private selectors used by public types.
Values outside the existing supported kinds (1, 2, 4, 8, and 16) are rejected;
oversized numeric selectors no longer silently become default INTEGER.
Test 65 covers imported selectors, local block selectors, function arguments
and results, arrays, derived components, and generic dispatch. Its native
O0/O2 and interpreter results agree with gfortran, including 64-bit values.
The batch suite also compiles it as separate module and program files.
Kind-selector expressions, kind-suffixed integer literals, and nondefault-kind
INTEGER PARAMETER values remain unsupported. Default-kind PARAMETER declarations
can now explicitly specify kind 4.

The sixth checkpoint accepts multiple deferred dimensions and lowers numeric
multidimensional arrays with known positive extents to column-major storage.
Local straight-line ALLOCATE, scalar fills, conforming array assignments,
whole-array output, SIZE(array[,dim]), and whole-array assumed-shape subroutine
arguments now work for these arrays. Dynamic scalar subscripts are checked
per dimension; invalid constant subscripts are rejected during compilation.
Shape mismatches and allocation rank mismatches are diagnosed before flattening.
Test 66 exercises allocated matrices and a tensor; test 67 checks a runtime
bounds failure that would otherwise alias another column. Native O0/O2 and
interpreter checks cover out-of-range indices in both dimensions.

This uses static storage specialization, not a runtime array descriptor ABI.
Dynamic/zero extents, DEALLOCATE/reallocation,
array lower bounds other than one, USE-associated multidimensional arrays,
and procedure calls in multidimensional
subscripts remain unsupported. Subscripts must have explicitly declared
scalar integer types of at most 64 bits. Existing initialization diagnostics
refer to the flattened element number. Numeric matrix elements are INTEGER
kinds 1/2/4/8, REAL, or DOUBLE PRECISION.

The seventh checkpoint adds scalar CHARACTER PARAMETER declarations, including
`character(len=*)` and `character(*)` inferred lengths, literal concatenation,
and constant CHAR/ACHAR calls with integer constant expressions. Explicit
lengths pad or truncate each initializer; bare CHARACTER has length one.
Imported and host-associated character constants retain their resolved lengths.
Test 68 checks module imports, host association, padding, truncation, zero
length, concatenation, and embedded newlines against gfortran, native O0/O2,
and the interpreter. The batch suite also builds it as two source files.

Character constant evaluation currently accepts non-NUL ASCII only; CHAR and
ACHAR accept codes 1 through 127. Character array parameters and assumed-length
variables/dummies are explicitly rejected. Runtime character intrinsics,
nonconstant or named length selectors, and general character arrays remain
outside this checkpoint.

The eighth checkpoint adds multidimensional section expressions with constant
bounds, strides, and scalar subscripts. Mixed scalar/range subscripts reduce
rank, and sections preserve column-major order. Whole-array assignment checks
the original section shapes before flattening and preserves overlapping RHS
values. Direct list-directed output and existing numeric reductions also accept
these sections, including empty sections. Assumed-shape subroutine specialization
substitutes proven constants in section bounds (for example a local assigned
from SIZE). Test 69 covers the consecutive-row difference from diff_matrix_real,
strides, mixed subscripts, rank-three arrays, empty reductions, and overlap,
with matching native O0/O2, interpreter, and gfortran results.

Dynamic section bounds/scalar subscripts, vector subscripts,
SIZE of sections, and section procedure arguments remain unsupported. Unsupported
section contexts are diagnosed rather than passed to the backend.

The ninth checkpoint adds ELEMENTAL functions, implicitly PURE, with optional
explicit PURE before or after ELEMENTAL. Arguments and results must currently
be explicitly declared numeric scalars; arguments require INTENT(IN). Existing
purity checks reject external I/O, dummy modification, and impure calls.
Scalar calls use the existing function implementation. Array calls are lowered
to scalar calls with temporary results, supporting scalar expansion, nested
calls, whole-array assignments, direct output, and reductions. Array argument
shapes are checked before multidimensional arrays are flattened. Test 70 covers
module imports, typed and RESULT functions, integer and double-precision results,
sections, matrices, empty arrays, and overlapping assignments. Native O0/O2 and
interpreter results agree with gfortran within printed numeric precision.

This is not full elemental procedure support: ELEMENTAL subroutines, IMPURE
ELEMENTAL, optional arguments, nonnumeric arguments/results, integer kind 16,
and elemental generic dispatch with array actual arguments remain unsupported.
Array expressions retain the existing static-shape and numeric-expression limits.
Procedure prefixes before the return type are supported; reordered type/prefix
forms such as `real elemental function` are not yet supported.

The tenth checkpoint implements a prerequisite for that FORALL statement:
assignment to constant array sections, including mixed scalar/range subscripts,
strided and reversed destinations, and conforming array expressions on the RHS.
Rank-one section assignment also now accepts array RHS values and constructors
of scalar elements. Original ranks/extents are checked before flattening, so
arrays with equal element counts but different shapes are rejected.
All RHS values are captured before any destination writes, preserving overlapping
assignments; scalar RHS expressions are evaluated once. Constant destination
indices preserve element-level definite-initialization checks.
Test 71 compares overlapping shifts/reversals, matrix and tensor destinations,
empty sections, scalar evaluation count, and a two-column version of the utility
column-scaling calculation against gfortran, native O0/O2, and the interpreter.
The lowering currently expands the destination elements at compile time; runtime
bounds/subscripts and general array-valued constructor elements remain unsupported.

The eleventh checkpoint implements single-statement FORALL with one explicitly
declared integer index (kind 4 or 8), constant bounds/stride, and numeric array
element or section destinations. Bounds can use PARAMETERs, SIZE, or constants
proven during assumed-shape subroutine specialization. Each iteration's result
is captured before any destination writes; duplicate destination elements are
rejected. The outer index variable is left unchanged, and its prior initialization
state is preserved. Calls in the assignment must be pure supported procedures.

Test 72 covers overlapping reversals, negative strides, empty iteration ranges,
section destinations, pure function calls, local arrays inside functions, and
the utility's column-scaling FORALL body in a specialized subroutine. Native
O0/O2 and interpreter output agree with gfortran. The batch suite also exercises
these procedures through a separate module file. Local arrays are now registered
in the undefined-name analysis, and FORALL indices are counted as used without
introducing runtime reads or writes of the enclosing variable.

FORALL constructs, multiple indices, masks, nested FORALL, runtime bounds,
data-dependent destination subscripts, nonnumeric destinations, and array
constructors in FORALL assignments remain unsupported. This implementation
expands iterations at compile time and is intended for statically sized cases.

The twelfth checkpoint adds a prerequisite for file input: advancing formatted
character-record READ from standard input, using `read(*,"(a)") text` or constant
unit 5. One explicitly declared scalar CHARACTER destination is supported.
Leading/trailing spaces and blank records are retained, short records are padded,
and long records are truncated while consuming the entire record. A final record
without a newline is accepted; EOF before any record is an error. Zero-length and
bare CHARACTER destinations are covered. The current string representation limits
input to non-NUL ASCII text; unsupported formats, destinations, and file units
are rejected explicitly. PURE procedures cannot use this external I/O.

The native list-directed reader now consumes the remainder of its last record,
so a subsequent formatted READ does not see a leftover newline. The interpreter
also ignores unused record text after a numeric input token. General list-directed
input (including multiple items in the interpreter) still needs further work.

`tests/io/record_read.f90` and `python tests/test_record_read.py` exercise native
O0/O2 and interpreter input, exact padding/truncation, CRLF, 12,000-character
records, mixed list-directed/formatted input, EOF, and invalid bytes. Successful
output is also compared with gfortran when installed. Build the debug compiler
first; set `F90C_TEST_COMPILER=target/release/f90c.exe` to test the release build.
The batch suite includes a silent compilation and input/output check.

The thirteenth checkpoint implements `read(text,*,iostat=ierr) nw` for one
explicitly declared scalar INTEGER destination (kinds 1, 2, 4, and 8). The internal
unit must be a scalar CHARACTER variable. IOSTAT is optional and, when present,
must be a distinct writable scalar INTEGER of kind 4 or 8. The scanner accepts
signed decimal integers, leading whitespace, comma/slash termination, null values,
and positive repetition counts (including repeated nulls). It validates the
entire first token and checks the destination kind's range before assignment.
A null value preserves the destination; malformed input gives positive IOSTAT,
and exhausted input gives negative IOSTAT. Error/EOF conservatively invalidates
the destination for initialization checking. Without IOSTAT, an input error
terminates the program. The supported internal READ is allowed in PURE procedures
with the usual restrictions on writable variables.

The scanner lowers to shared AST control flow plus a bounded byte-read primitive,
so native and interpreted execution use the same conversion and overflow logic.
Boundary testing also exposed and fixed interpreter integer comparisons that
previously lost precision by converting both operands to floating point. Scoped
usage analysis now recognizes source variables referenced in generated blocks.

Test 73 covers conversion, null/repeated values, malformed tokens, all supported
integer boundaries, optional IOSTAT, and a PURE function using internal input.
`python tests/test_internal_read.py` compares native O0/O2 and interpreter output
with the golden result and gfortran (when installed), checks failed/null reads
against initialization guards, and combines standard-input record reads with
internal conversion. It uses the same F90C_TEST_COMPILER override as the record
input suite. Multiple input items, array destinations, real/logical/character
conversion, internal arrays of records, arbitrary unit expressions, and other
I/O control specifiers remain unsupported.

The fourteenth checkpoint adds standard-output WRITE with positional controls:
`write(*,*) items`, `write(6,*) items`, and the literal formats already supported
by PRINT. Named integer PARAMETERs equal to 6 are accepted as units. List-directed
WRITE uses the existing PRINT lowering, including whole arrays and supported
sections. Empty lists produce a blank record. Other units, internal-file output,
keyword controls, format labels/variables, and extended formatting remain outside
this subset. A literal format must satisfy the existing output-format validator.

WRITE is a contextual keyword, preserving variables, arrays, and procedures named
`write`. Empty lists are recognized at logical statement boundaries, including
comments, semicolons, continuations, and single-line IF statements. Output still
checks initialized values, and external WRITE is forbidden in PURE procedures.

The utility's following bare STOP also now parses and terminates the process with
status zero, including when reached inside a block, loop, or called subroutine.
Native main-program generation now honors terminated blocks instead of appending
instructions after a terminating trap. This checkpoint covers STOP without a code;
coded STOP syntax/runtime support is not extended here.

Test 74 and `python tests/test_write.py` cover list-directed/formatted output,
arrays/sections, blank records, contextual names, and termination. Native O0/O2
output matches the golden result; values and formatted fields agree with gfortran
and the interpreter. The interpreter retains its existing extra trailing zeros
for list-directed real output. The batch suite checks silent compilation with
the rebuilt release compiler. The test script supports F90C_TEST_COMPILER like
the input scripts.

The full utility program still does not compile. Executing
`read (iu,"(a)") text` still requires file-unit support (OPEN/CLOSE and unit-to-stream
mapping). The next parser error is the implied-DO output list in
`write (*,*) (trim(words(i)) // " ",i=1,nread_)`.
The internal READ of `nw,words` also needs multiple items and character arrays.
Its character array parameters remain unsupported.
Further blockers include user-defined dotted
operators such as `.in.` and `.exclude.`. The utility library also exercises
features beyond the currently implemented array, character, and I/O subsets.

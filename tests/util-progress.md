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
first; set `F90C_TEST_COMPILER=target/release/rfort.exe` to test the release build.
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

The fifteenth checkpoint adds implied-DO lists to list-directed PRINT and
standard-output WRITE, including multiple items, nested loops, runtime bounds,
and positive/negative runtime steps. Controls are evaluated once on loop entry;
empty loops and final control-variable values follow ordinary DO semantics.
The control variable must be an explicitly declared scalar INTEGER of kind 4
or 8. Bounds and steps currently accept integer literals, scalar integer
variables, arithmetic, and selected integer intrinsics. General user-function
bounds, formatted/input implied-DOs, and constructor implied-DOs remain outside
this subset and are rejected. A nested implied-DO cannot reuse an active index.

Lowering uses ordinary AST assignments/loops with output fragments, retaining a
single output record and the existing list-directed spacing rules, including
adjacent character items. Whole numeric arrays and supported constant sections
can appear alongside loops. Initialization checks run at each actual read,
reporting the original source line and the first unset array element encountered.
A constant zero step is a compile-time error; a runtime zero step or overflowing
64-bit control-variable increment terminates with a diagnostic.

The character-output regression also exposed native TRIM/concatenation bugs.
Direct output now streams concatenation operands, and native TRIM seals its
control-flow blocks and terminates the result at the trimmed length, including
an all-blank argument. This covers the scalar pattern `trim(word)//" "`; it does
not add general character-array storage or assumed-length character dummies.

Test 75 and `python tests/test_output_implied_do.py` cover output records,
positive/negative/empty/nested loops, dependent bounds, subroutine output,
runtime controls, initialization errors, invalid forms, and overflow. Native
O0/O2 and interpreted output agree, and the successful fixture agrees with
gfortran apart from numeric field widths. The script supports F90C_TEST_COMPILER.
The batch suite also verifies silent compilation and execution of the fixture.

At the fifteenth checkpoint, the next parser blocker was
`backspace(iu)` (combined source line 808; the error is reported at the following
READ on line 809). File-positioning statements require external-file support.
Executing `read (iu,"(a)") text` still requires file-unit support (OPEN/CLOSE and
unit-to-stream mapping). The internal READ of `nw,words` also needs multiple
items and character arrays. Its character array parameters remain unsupported.
The array forms of user-defined dotted operators such as `.in.` and
`.exclude.` remain blockers. The utility library also exercises features beyond the currently
implemented array, character, and I/O subsets.

The compiler executable is now `rfort` (`rfort.exe` on Windows), built with
`cargo build --release --bin rfort`. Use `rfort.bat` for silent Windows
compilation; `f90c.bat` forwards to it for compatibility. The source-combining
helper and `.f90c-sources.f90` suffix retain their existing names. Test scripts
use `target/debug/rfort.exe`; `RFORT_TEST_COMPILER` overrides that path, and the
legacy `F90C_TEST_COMPILER` variable is also accepted.

The native driver now owns source grouping and linking. `rfort` compiles ordered
source groups directly; both Windows wrappers delegate to it. Windows toolchain
paths are discovered at runtime, and Linux/macOS use explicit system C compiler
paths with portable runtime adapters. See README.md for overrides and platform
validation limits. `tests/test_driver.py` exercises the executable directly,
including a copied executable outside the checkout. `tests/testsuite.rs` now
passes module dependency sources together and no longer runs `git clean`.

The sixteenth language checkpoint adds expressions using user-defined dotted
operators. Unary defined operators bind more tightly than exponentiation;
binary defined operators bind below intrinsic operators and associate left to
right. Names are case-insensitive. Module interfaces select scalar functions by
operand type, kind and arity, with INTENT(IN), nonoptional dummy arguments.
USE ONLY, private/public access, re-export and host association use the existing
operator visibility rules. Mismatched and ambiguous overloads are diagnosed.
Intrinsic-operator overload restrictions are unchanged.

Constant/kind resolution now has a separate early entry point for overload
selection. PURE validation still runs after operator calls and derived values
are lowered, so it checks the actual selected function instead of a parser
placeholder. It rejects calls to impure operator implementations from PURE
procedures.

Test 76 and `python tests/test_defined_operators.py` cover unary/binary
precedence, integer/real dispatch, nested calls, re-export, and PURE calls.
Native O0/O2 and interpreter output match gfortran. Unit tests cover invalid
interfaces, inaccessible/missing overloads, type/rank mismatch, and the explicit
rejection of array operands/results: the existing function lowering cannot yet
pass these correctly. Thus this checkpoint does not implement the array-based
`.in.` or character-array `.exclude.` operations used in util.f90. The full
`kind.f90 util.f90 xutil.f90` build still stopped at BACKSPACE at this checkpoint;
the user source files have not been modified.

The seventeenth checkpoint adds read-only sequential formatted external files.
OPEN requires explicit STATUS='OLD', ACTION='READ', and a constant FILE name;
optional ACCESS='SEQUENTIAL' and FORM='FORMATTED' are accepted. Unit numbers
must be in 0..1023 excluding preconnected units 5/6. The supported `(A)` READ
accepts one scalar CHARACTER destination, pads/truncates it, and consumes the
whole record. BACKSPACE supports repeated backward positioning and is a no-op
at the beginning; REWIND returns to the beginning. CLOSE permits reopening a
unit and is a no-op for an unconnected unit. Both parenthesized positioning
controls and bare BACKSPACE/REWIND unit syntax are accepted. These words remain
available as ordinary variable and procedure names.

Native code calls nonvariadic host C file functions, using 64-bit offsets;
the interpreter uses Rust file/seek operations. LF, CRLF, long and blank
records, and a final record without newline are covered. File connections are
shared across procedures in a compiled source group. Separate-object sharing,
external list-directed numeric/array reads, file writes, NEWUNIT, IOSTAT, and
other file modes remain outside this subset. Unsupported controls are rejected;
open/position/read failures terminate with a runtime diagnostic. PURE procedures
cannot perform these operations, and file-unit expressions are checked for
initialization before use.

Test 77 and `python tests/test_file_io.py` compare native O0/O2 and interpreter
output with gfortran, including repeated backspaces, multiple units, reopening,
truncation, line endings, and missing/unconnected files. The batch suite also
checks the new fixture. The original utility files remain unchanged. Their
combined source now parses past BACKSPACE and stops at computed CHARACTER
length in `join`: `character(len=(size(words)-1)*len(sep)+sum(len_trim(words)))`,
combined-source line 852. Full utility compilation is still incomplete.

The eighteenth checkpoint parses computed CHARACTER lengths and resolves the
constant subset: integer arithmetic, named parameters, constant LEN/LEN_TRIM,
MIN/MAX, ABS, and positional `character(n)`. Negative lengths become zero;
overflow, invalid types, and runtime-dependent lengths produce diagnostics.
LEN of a fixed-length scalar does not read its contents. CHARACTER arrays and
CHARACTER-valued function results are explicitly rejected pending storage/ABI
support, even though their length syntax can now be parsed.

Scalar assignment now preserves the declared length instead of propagating an
unpadded literal. Native and interpreter paths support literal/variable copies,
concatenation, and TRIM, including self-assignment and overlapping expressions.
Sources are evaluated before destination writes; uninitialized reads retain their
checks. The default CHARACTER length is one. Regression 07 now expects the
trailing spaces required by its declared length, verified with gfortran.

Test 78 and `python tests/test_character_lengths.py` compare the interpreter and
native O0/O2 with gfortran; the batch suite covers the new fixture. The utility
source files remain unchanged. Their combined source parses past the computed
length in `join` and now stops at `end function concat`, combined-source line
945, because the preceding ELSE IF chain is not supported by the current grammar.
Full utility compilation is still incomplete; the runtime lengths and character
array/function results in these routines will also need implementation.

Validation on Windows x64: 69 Rust unit tests, all 78 native regression fixtures,
6 focused CHARACTER tests with both debug and release compilers, 25 batch tests,
and the 21 formatted/internal input, WRITE, implied-DO output, and external-file
checks passed. The release executable was rebuilt. Linux/macOS execution has
not been validated in this checkpoint.

The nineteenth checkpoint adds ELSE IF and ELSEIF branch chains, lowered to
nested IF nodes with a single closing END IF. Only reached conditions are
evaluated, and an optional final ELSE handles the unmatched case. A separate
ELSE statement followed by IF remains a nested block requiring two END IFs;
newlines, semicolons, comments, and continuation markers preserve that distinction.
Continuation handling now accepts intervening blank/comment lines before an
optional leading ampersand. ELSEIF remains usable as a variable/procedure name.

Test 79 and `python tests/test_else_if.py` cover every branch, no final ELSE,
nested blocks, both spellings, continued statements, malformed chains, and
side-effecting conditions compared with gfortran in the interpreter and native
O0/O2. The batch suite covers the same fixture.

The original utility sources remain unchanged. Parsing now passes `concat`
and reaches the two-index FORALL in `munge`, combined-source line 1033:
`forall (ix=1:size(xx),iy=1:size(yy)) ...`. The existing FORALL subset accepts
only one index. Full utility compilation remains incomplete.

Validation on Windows x64: all 70 Rust unit tests, 79 native regression fixtures,
3 focused ELSE IF tests with debug and release compilers, and 26 batch tests
passed. The release executable was rebuilt, and the next utility parser error
was confirmed with that executable. Linux/macOS execution remains untested.

The twentieth checkpoint extends single-statement FORALL to multiple explicitly
specified integer indices. Independent constant triplets form a Cartesian
product; all right-hand sides are captured before any destination writes, even
across different index combinations. Two- and three-index cases, transposition,
array-section snapshots, negative strides, empty products, and specialized PURE
array subroutines are covered. Index variables retain their surrounding values
and are not initialized by the construct. Specialization now protects every
FORALL index from replacement by its enclosing value.

Duplicate indices, repeated destination elements across the entire product,
zero strides, invalid destination bounds, nonconstant/dependent triplets, and
iteration-count overflow are diagnosed. Existing limits remain: numeric arrays,
static shapes/bounds, one assignment, no masks or block FORALL, and compile-time
expansion intended for small statically sized cases. Character destinations
such as the one in `munge` will still require character-array support.

Test 80 and `python tests/test_multi_forall.py` compare interpreter and native
O0/O2 results with gfortran and check that uninitialized RHS reads cannot be
hidden by earlier writes. The batch suite covers the new fixture. The original
utility sources remain unchanged. Their combined source now parses past the
multi-index FORALL and stops at `yy = (/(xx,i=1,n)/)` in `repeat_char`, line 1051:
old-style array constructors and constructor implied-DO need further work.
Full utility compilation remains incomplete.

Validation on Windows x64: 71 Rust unit tests, all 80 native regression fixtures,
2 focused multi-index FORALL tests with debug and release compilers, and 27
batch tests passed. The release executable was rebuilt and the next utility
parser error confirmed with it. Linux/macOS execution remains untested.

The twenty-first checkpoint accepts both `[ ... ]` and `(/ ... /)` array
constructors, including whitespace in old-style delimiters. Lexer normalization
preserves ordinary division and OPERATOR(/) interfaces. Constant-bound implied-DOs
expand in element order, including nested loops with bounds depending on outer
indices, signed strides, empty ranges, mixed scalar/loop items, and nested
constructors. Numeric PARAMETER array initializers and PURE procedures use the
same expansion. Assignment retains the existing capture-before-write behavior,
so reversing an array through a constructor reads its original values.

This subset requires implied-DO indices declared locally as scalar INTEGERs;
the surrounding variables are neither read nor defined. Runtime bounds, array-
valued items, and CHARACTER-array storage remain unsupported. Duplicate active
indices, zero strides, undefined source reads, shape mismatches, and excessive
compile-time expansion produce diagnostics. Expansion is limited to 100,000
steps per constructor (iterations and scalar items count toward that limit).
A missing scalar-item check in whole-array assignment was also fixed, preventing
array-valued constructor items from silently taking a scalar storage path.

Test 81 and `python tests/test_array_constructors.py` compare the interpreter,
native O0/O2, and gfortran, and cover invalid/unsupported forms. The batch suite
includes the new fixture. The original utility sources remain unchanged.
Parsing now passes the old-style constructor in `repeat_char` and stops at
`lbound(ivec,dim=1)` on combined-source line 1115: keyword intrinsic arguments
need further work. Full utility compilation remains incomplete.

Validation on Windows x64: 72 Rust unit tests, all 81 native regression fixtures,
2 focused constructor tests with debug and release compilers, and 28 batch tests
passed. The release executable was rebuilt and the next utility parser error
confirmed with it. Linux/macOS execution remains untested.

The twenty-second checkpoint adds LBOUND and UBOUND for supported statically
shaped arrays and sections. Constant DIM selects a scalar; omission returns a
rank-one bounds vector usable in assignment, list-directed output, and reductions.
Whole-array declarations currently start at one; sections are rebased to one,
with zero upper bounds for empty dimensions. Shape-only inquiries do not read
or initialize array contents. Specialized assumed-shape subroutines with whole-
array actual arguments and PURE procedures are covered.

ARRAY/DIM/KIND keywords are parsed and reordered for LBOUND, UBOUND, and SIZE.
Unknown/duplicate keywords, positional arguments after keywords, missing ARRAY,
scalar arguments, invalid dimensions, runtime DIM, and nondefault result kinds
are diagnosed. Only default INTEGER KIND=4 is supported. Existing storage and
specialization limits remain; this does not add arbitrary declared lower bounds,
dynamic descriptors, section actual arguments, or inquiry PARAMETER initializers.

Test 82 and `python tests/test_bound_inquiries.py` compare interpreter/native
O0/O2 results with gfortran for scalars, vectors, sections, empty extents, PURE
procedures, and assumed-shape dummies, and exercise the diagnostics. The batch
suite covers the fixture. Original utility sources remain unchanged. The combined
source now parses past both LBOUND and UBOUND in the slice routines and stops at
`write (outu_,fmt_time_) ...`, combined-source line 1212, because formatted WRITE
currently requires a literal format. Full utility compilation remains incomplete.

Validation on Windows x64: 74 Rust unit tests, all 82 native regression fixtures,
3 focused inquiry tests with debug and release compilers, and 29 batch tests
passed. The release executable was rebuilt and the next utility parser error
confirmed with it. Linux/macOS execution remains untested.

The twenty-third checkpoint accepts expression formats in WRITE and PRINT.
Supported constant CHARACTER expressions, including imported PARAMETERs,
padded named formats, and concatenation, are folded before output lowering.
WRITE without output items also accepts named formats. List-directed WRITE now
uses a distinct internal marker so numeric format labels (including zero) cannot
silently become list-directed output. Runtime character formats and numeric
labels produce explicit unsupported-feature diagnostics. Existing descriptor,
output-unit, and format-reversion limits remain.

Test 83 and `python tests/test_named_formats.py` compare exact formatted output
in the interpreter and native O0/O2 builds with gfortran, and cover unsupported
formats, units, and mismatched output lists. The batch suite covers the fixture.
Original utility sources remain unchanged. Parsing now passes the variable
format in `print_wall_time_elapsed` and stops at `ADVANCE="no"` on combined-source
line 1261. That variable format still needs runtime formatting support, and
full utility compilation remains incomplete.

Validation on Windows x64: 74 Rust unit tests, all 83 native regression fixtures,
2 focused format tests with both debug and release compilers, and all 30 batch
tests passed. The release executable was rebuilt and the next utility parser
error confirmed with it. Linux/macOS execution remains untested.

The twenty-fourth checkpoint adds runtime scalar CHARACTER formats to WRITE
and PRINT, including variable formats, formats read from input, concatenation,
and TRIM. A shared allocation-free parser keeps constant-format validation,
interpreter execution, and the native runtime aligned. Supported descriptors
are A, I, F, L, quoted literals, repeat counts, nX spacing, and / record breaks.
The empty format () supports an empty output list. Existing width/precision
and repeat limits remain. Scalar output items are evaluated once, and syntax,
item counts, and descriptor types are validated before statement output starts.
Malformed runtime formats exit nonzero with an explicit runtime diagnostic.
Uninitialized format variables retain the existing initialization checks.

The native helper is Rust without the standard library, built by build.rs for
the compiler target and embedded as a static archive. rfort link supplies it
automatically on Windows/Linux/macOS; users need no separate runtime files or
Rust installation to run generated programs. Constant native formats use this
helper as well. Numeric labels, nested groups, reversion, ADVANCE, formatted
array output, and existing CHARACTER procedure-argument limitations remain.

Test 84 and python tests/test_runtime_formats.py cover changing formats, input
formats, concatenation/TRIM, literal quoting, padding, numeric field overflow,
spacing, record breaks, empty output lists, invalid descriptors, count/type
mismatches, and compile-time initialization/type checks. Formatted output is
compared exactly with gfortran. Named-format tests were updated to remove the
now-obsolete runtime-format rejection cases. Original utility sources remain
unchanged; their next parser error is still ADVANCE="no" on combined-source
line 1261. Full utility compilation remains incomplete.

Validation on Windows x64: 74 Rust unit tests, all 84 native regression fixtures,
3 runtime-format and 2 named-format tests with both debug and release compilers,
and all 31 batch tests passed. The release executable was rebuilt and the next
utility parser error confirmed with it. Linux/macOS execution remains untested.

The twenty-fifth checkpoint adds ADVANCE='no' and ADVANCE='yes' to formatted
standard-output WRITE, including writes with empty output lists and runtime
format strings. ADVANCE accepts constant CHARACTER expressions, including
named parameters and concatenation. Duplicate controls, invalid values, unknown
WRITE controls, and ADVANCE on list-directed output are diagnosed. Runtime
ADVANCE values remain unsupported. Other existing formatted-output limits stay
in place.

Nonadvancing writes omit the final newline and flush stdout so prompts are
visible before input. Subsequent writes continue on the same line. Final
nonadvancing stdout output remains unterminated at program exit or STOP,
matching the gfortran reference tested here. Explicit / descriptors still
start a new record. The native helper uses a new entry point with an advancing
flag; the old advancing-only ABI remains available for previously emitted
objects.

Test 85 and python tests/test_nonadvancing_output.py exercise joined records,
loops, empty output lists, mixed formatted/list-directed output, record breaks,
constant ADVANCE expressions, STOP, and invalid controls. A pipe-based prompt
test waits for output before supplying input to verify flushing in interpreter
and native O0/O2 modes. The output fixture is compared exactly with gfortran.
Original utility sources remain unchanged. Parsing now passes ADVANCE='no'
and stops at elemental subroutine set_optional_real on combined-source line
1356. Full utility compilation remains incomplete.

Validation on Windows x64: 74 Rust unit tests, all 85 native regression fixtures,
4 nonadvancing-output and 3 runtime-format tests with debug and release compilers,
and all 32 batch tests passed. The release executable was rebuilt and the next
utility parser error confirmed with it. Linux/macOS execution remains untested.

The twenty-sixth checkpoint adds scalar ELEMENTAL subroutine calls. ELEMENTAL,
PURE ELEMENTAL, and ELEMENTAL PURE prefixes are accepted, and ELEMENTAL implies
PURE. Declaration and purity checks require supported scalar dummy types and
explicit INTENT. The existing call specialization pass now handles these
routines, resolves omitted trailing OPTIONAL arguments, folds PRESENT guards,
and rejects references to absent arguments in surviving branches. PURE callers
may omit optional arguments when calling another PURE subroutine. Nested calls
retain the types of specialized input temporaries.

All four elemental setter implementations from util.f90 are exercised in test 86:
set_optional_real (DOUBLE PRECISION), set_optional_integer, set_optional_logical,
and set_optional_character. Scalar assumed CHARACTER dummy lengths come from
the actual arguments, preserving padding, truncation, and zero lengths. INOUT
updates and nested elemental calls are covered too. Input copies and output
initialization checks use the existing specialization machinery.

This checkpoint supports direct positional calls with scalar actuals. Array
actuals, generic OPTIONAL dispatch, keyword calls, recursive calls, general
actual expressions, and the separate allocatable setter family need further
work. Explicit-length CHARACTER dummies currently require matching actual
lengths. Invalid types, missing required arguments, absent optional reads,
invalid PRESENT operands, and purity violations produce diagnostics.

python tests/test_elemental_setters.py compares interpreter and native O0/O2
output exactly with gfortran and exercises diagnostics. The batch suite includes
the fixture. Original utility sources remain unchanged. Parsing now passes all
four elemental set_optional routines and stops at the negative-step DO loop in
last_row_all_true, combined-source line 1442. Full utility compilation remains
incomplete.

Validation on Windows x64: 74 Rust unit tests, all 86 native regression fixtures,
2 focused setter tests with debug and release compilers, and all 33 batch tests
passed. The release executable was rebuilt and the next utility parser error
confirmed with it. Linux/macOS execution remains untested.


## Checkpoint 27: explicit constant DO steps

The parser accepts a third DO control expression. A lowering pass snapshots
both bounds before assigning the control variable, selects the comparison for
the step's sign, and advances on normal iterations and CYCLE. EXIT and nested
loop control retain their targets. Steps must currently be constant nonzero
INTEGER expressions; bounds and the declared control variable must be scalar
INTEGER. Increment overflow in 64-bit storage reports ERROR STOP rather than
wrapping. Runtime-variable steps and named loops remain unsupported.

Native SELECT CASE now preserves enclosing loop targets and compiles every
branch even when another branch terminates. A selector with no matching case
and no default continues after SELECT. Fixture 87 covers these paths along with
positive/negative/named steps, empty loops, final indices, bound snapshots,
nesting, PURE functions and assumed-shape INTEGER subroutine calls.
python tests/test_do_steps.py checks interpreter/native O0/O2 against gfortran,
invalid controls, and positive/negative increment overflow.

Original utility sources remain unchanged. The full build now parses past the
reverse loop and stops at `where (.not. ygood) yy = ydefault`, combined-source
line 1743. Extracting last_row_all_true also exposes the separate unsupported
multidimensional LOGICAL array limitation; this checkpoint does not claim that
routine or the full utility module compiles yet.

Validation on Windows x64: all 74 Rust unit tests, 87 native regression
fixtures, 3 focused tests with both debug and release compilers, and 34 batch
integration tests passed. The release executable was rebuilt and the next
utility parser error confirmed with it. Linux/macOS execution remains untested.


## Checkpoint 28: ofort compatibility baseline and rank-one WHERE

Added tests/run_ofort_compat.py with an 18-case external selection and recorded
before/after reports in tests/ofort. The original ofort checkout is read only;
the runner uses separate temporary working directories, execution timeouts,
gfortran references, and native O0/O2 checks. It distinguishes compilation,
execution, crashes/timeouts and wrong output, and records source/compiler hashes.
The initial baseline had 3 passes and 15 compilation failures.

WHERE now supports single assignments and block constructs with plain/masked
ELSEWHERE, capturing each mask before modifying its inputs. Every assignment
captures only selected RHS values before writing destinations, including
reversed/overlapping RHS sections. ELEMENTAL array calls preserve masked
evaluation; non-elemental reductions evaluate their full arguments. LOGICAL
rank-one arrays now have interpreter and native storage, indexed reads/writes,
constructors, assignments and output. Previously unsupported logical section
assignment tests were updated to verify acceptance; invalid numeric reductions
on logical arrays are still rejected.

xwhere_basic and xwhere_elsewhere_forms now pass, bringing the sample to 5/18.
The other 13 still fail compilation. This is a selected development sample, not
a suite-wide compatibility percentage. Nested WHERE, multidimensional masks,
section destinations, and CHARACTER destinations remain unsupported. Local
fixture 88 tests mask/RHS snapshots, inactive uninitialized and division-by-zero
operands, ELEMENTAL calls, ELSEWHERE precedence, LOGICAL output, and empty arrays.

The full utility build now parses past WHERE and stops at the keyword actual
argument stop_error=.true. in a CALL, combined-source line 1963. Utility sources
are unchanged, and full compilation remains incomplete.

Validation on Windows x64: 74 Rust unit tests, all 88 native regression fixtures,
3 focused WHERE tests with debug and release compilers (including gfortran
comparison), 2 compatibility-runner tests, and all 35 batch tests passed. The
release executable was rebuilt; its external ofort scan confirms 5 passes and
13 compilation failures at both O0/O2, with all reference runs successful.
Linux/macOS execution remains untested.


## Checkpoint 29: explicit fixed array lower bounds

Declarations now accept constant INTEGER lower:upper dimensions, with named
parameters resolved before normalization. The new array_bounds pass preserves
whole-array LBOUND/UBOUND values and rebases indices/sections to existing
one-based storage before argument specialization and multidimensional flattening.
Each dynamic explicit-bound index is evaluated once in a generated guard that
checks the original range before subtraction, preventing overflow or cross-column
aliasing. Constant bad indices and sections receive original-range diagnostics.
Extent arithmetic is checked using wider intermediates.

Fixture 89 exercises negative/zero/positive lower bounds, column-major dynamic
indexing, explicit reversed sections, omitted endpoints, section assignments,
whole/section bounds inquiries, empty rank-one arrays, WHERE, LOGICAL elements,
local scope bounds, and actual/dummy rebasing. The focused Python tests compare
interpreter and native O0/O2 output with gfortran and check diagnostics/runtime
range failures, including an extreme invalid INTEGER index.

The external selection remains at 5/18 passes. Its four explicit-declaration
parser failures now advance to unsupported WHERE mask/shape handling and
array-intrinsic keyword arguments. These are recorded in after-explicit-bounds.json;
this checkpoint does not claim that those full examples compile. Runtime-dependent
bounds, explicit assumed-shape lower bounds and allocatable bounds remain outside
this subset; existing zero-extent multidimensional restrictions also remain.

Validation on Windows x64: all 74 Rust unit tests, 89 native regression fixtures,
36 batch tests, and 3 focused bounds tests with both debug and release compilers
passed. The focused execution test also matches gfortran. The release executable
was rebuilt and the refreshed external report confirms 5 passes / 13 compile
failures at both O0/O2. All 18 reference builds/runs succeeded. The full utility
build still stops at the existing CALL keyword argument at combined line 1963.
Linux/macOS execution remains untested.


## Checkpoint 30: numeric reduction keywords and masks

SUM, PRODUCT, MINVAL and MAXVAL now normalize ARRAY/DIM/MASK keywords,
rejecting duplicates, unknown names, missing ARRAY and positional arguments
following keywords. Fixed rank-one DIM=1 reduces to a scalar. Without DIM,
whole-array reductions accept scalar LOGICAL masks or conformable array masks;
shape checks run before multidimensional arrays are flattened. A positional
mask with omitted DIM still requires spelling MASK=. Runtime DIM and
multidimensional DIM results are explicitly rejected.

The reduction loop updates its accumulator only where MASK is true. Integer
MINVAL/MAXVAL initialization now respects the input kind, including empty and
all-false reductions; this also removes the old oversized-literal warnings from
fixture 28. Fixture 90 covers reordered keywords, masks, sections, explicit
bounds, REAL reductions, whole matrices and empty arrays. The focused Python
suite compares interpreter and native O0/O2 output with gfortran and checks
invalid DIM, mask type/shape and keyword combinations. The Windows wrapper
suite includes the same fixture.

The external 18-case sample remains at 5 passes / 13 compile failures at O0/O2.
The full reductions probe also needs COUNT keywords, multidimensional DIM
results, constructors in expressions and other array operations, so this
checkpoint is a tested smaller reproduction rather than a claim that the full
probe works. The xutil CALL-keyword blocker remains unchanged.

Validation on Windows x64: 74 Rust unit tests, 90 native fixtures, 37 batch
integration tests and both focused tests passed. Focused tests passed with debug
and rebuilt release compilers, exercising the interpreter plus native O0/O2 and
gfortran comparison. The refreshed release ofort report is
`target/ofort-masked-release.json`; all 18 reference builds/runs succeeded.
Linux/macOS execution remains untested.


## Checkpoint 31: COUNT and logical reduction arguments

COUNT now lowers LOGICAL arrays/expressions to a counting loop with INTEGER(4)
or INTEGER(8) result. COUNT, ANY and ALL accept positional/keyword MASK and
constant rank-one DIM=1; COUNT also accepts constant KIND=4/8. Reordered keywords,
parameter-valued DIM/KIND and calls within PURE procedures are supported.
Normalized internal intrinsic names keep argument rewriting idempotent when
procedure specialization revisits expressions. Empty arrays produce COUNT=0,
ANY=false and ALL=true without reading an element. Invalid keyword combinations,
mask types, scalar masks, dimensions and unsupported/nonconstant kinds fail.

Fixture 91 compares interpreter and native O0/O2 against gfortran, including
sections, explicit bounds, numeric matrix comparisons reduced without DIM,
KIND=8 arithmetic and pure functions with local arrays. Matrix DIM results still
need separate lowering and shape preservation; the full ofort reductions case
is not yet supported.

During testing, array arguments to functions produced wrong results in both
interpreter and native execution. A SUM-only reproduction also fails in the
previous release, establishing that this is an existing bug. It is retained in
`tests/pending/function_array_reduction.f90` (expected output 15), outside the
passing numbered suite, for the next checkpoint.

Validation on Windows x64: 74 Rust unit tests, 91 native fixtures, 38 batch tests,
and 2 focused tests passed. Focused interpreter/native O0/O2 checks match gfortran
with both debug and rebuilt release compilers. The release ofort report at
`target/ofort-logical-reductions.json` remains 5/18 passes (all reference cases
build/run); the full reductions case now gets past COUNT keywords and stops at
unsupported integer HUGE. Linux/macOS execution remains untested.


## Checkpoint 32: fixed-size array inputs to pure functions

The pending SUM reproduction now produces 15 in both interpreter and native
O0/O2 execution. Previously the scalar function ABI silently treated an array
actual as a scalar and left the dummy array unrelated to the caller's data.
The new function_arrays pass expands fixed-size array parameters/actuals into
scalar element parameters and reconstructs a private local array at function
entry. This retains ordinary function expression evaluation, including repeated
DO WHILE conditions, nested calls and returns, across both execution paths.
Compiler-generated parameters no longer receive unused-parameter warnings.

The deliberately bounded subset requires PURE, numeric/LOGICAL INTENT(IN) array
dummies, fixed exactly matching shapes, whole-array actuals and all arguments.
Default INTEGER and INTEGER(KIND=4) are compatible. Different original lower
bounds are handled after bounds normalization; multidimensional transfer uses
Fortran column-major order. Empty rank-one arrays are supported. Each array dummy
is limited to 1024 elements to bound scalar ABI expansion. Because all elements
are copied, actual arrays must be fully initialized. Assumed-shape,
OPTIONAL/ALLOCATABLE, non-PURE, writable and section/expression actual forms are
not implemented; supported-path constraints are diagnosed before code generation.

The pending file is promoted into numbered fixture 92, which adds multiple and
mixed scalar/array arguments, INTEGER/REAL/DOUBLE PRECISION/LOGICAL inputs,
matrices, empty arrays, explicit bounds and nested/repeated calls. The focused
Python test compares all three execution modes with gfortran and checks invalid
forms. The same fixture is included in the Windows batch integration suite.

Validation on Windows x64: 74 Rust unit tests, 92 native fixtures, 39 batch tests
and 3 focused tests passed. The focused tests passed with both debug and rebuilt
release compilers; execution matches gfortran at interpreter/native O0/O2 and
undefined input elements remain diagnosed. The external sample remains 5/18
passes at O0/O2; refreshed report: `target/ofort-function-arrays.json`.
Linux/macOS execution remains untested.


## Checkpoint 33: integer HUGE and empty/LOGICAL matrices

HUGE now returns 127, 32767, 2147483647 and 9223372036854775807 for INTEGER kinds
1, 2, 4 and 8. Default-integer constant expressions fold early, including HUGE(0)
in PARAMETER initializers. Typed scalar/array inquiries lower to kind-preserving
compiler temporaries without reading the argument. KIND now reports the actual
integer kind and KIND(HUGE(variable)) folds without unnecessary temporaries.
INTEGER(16) HUGE fails explicitly rather than returning a truncated limit; TINY
continues to reject integer arguments.

Fixture 93 covers limits, signed lower limits, literal expressions, PARAMETER
folding, result kinds and empty arrays. Focused checks compare interpreter and
native O0/O2 with gfortran, verify diagnostics, and confirm that an undefined
argument can supply its type without an uninitialized read. General variable
expressions and typed-variable inquiries in PARAMETER initializers still need
broader compile-time type inference.

Fixed-shape multidimensional arrays now accept zero extents and LOGICAL element
types. Existing column-major lowering and guarded subscripts handle empty storage
without an element read, including sections and whole-array reductions. Fixture
94 covers zero extents in several axes/ranks, explicit bounds, numeric/logical
empty identities, logical matrix assignment/output/sections and COUNT/ANY/ALL.
Its focused tests also check both constant and runtime invalid element access.

The ofort reductions case gets past integer HUGE and zero-extent/LOGICAL matrix
declarations. It now reaches the explicit rejection of multidimensional DIM
reductions; those array-valued results and the other remaining array operations
are subsequent work. This checkpoint does not claim the full probe compiles.

Validation on Windows x64: 74 Rust unit tests, 94 native fixtures, 41 batch tests
and 5 focused tests passed. Both focused suites pass with debug and rebuilt
release compilers and compare interpreter/native O0/O2 output with gfortran.
The refreshed report `target/ofort-huge-matrices.json` remains at 5/18 passes;
all 18 reference builds/runs succeeded. The reductions probe now stops at
`sum: DIM reductions currently require a rank-one array`. Linux/macOS execution
remains untested.


## Checkpoint 34: multidimensional DIM reductions

SUM, PRODUCT, MINVAL, MAXVAL, COUNT, ANY and ALL now reduce one constant axis
of a fixed-shape array. Shape inference removes that axis before storage is
flattened, so assignments and masks retain shape checks and bound inquiries
see the correct result shape. Lowering emits one accumulator loop per result
element using column-major input indexing. Masks guard element reads; empty
axes use the existing reduction identities and zero-size results do no work.
Runtime DIM remains unsupported.

This exposed an interpreter bug: INTEGER division returned a real value,
which broke computed array subscripts. Integer operands now produce an integer
quotient truncated toward zero; invalid division reports a runtime error.

Fixture 95 covers numeric and logical reductions, explicit lower bounds,
empty numeric/logical axes, nested reductions, masks and result bounds.
The focused suite adds rank-three arrays, reversed/strided sections, REAL and
DOUBLE PRECISION values, shape diagnostics, integer division in subscripts,
and masked undefined input elements. Interpreter and native O0/O2 execution
are compared with gfortran.

Validation on Windows x64: 74 Rust unit tests, 95 native fixtures, 42 batch
checks, and 12 focused tests passed. The three reduction suites (7 tests) also
pass with the rebuilt release compiler. The updated fixture was rechecked in
the interpreter, native O0/O2 and gfortran after adding empty logical axes.
Linux/macOS execution remains untested.

The external sample remains at 5/18 passes at O0/O2, with all 18 reference runs
successful (`target/ofort-dim-reductions.json`). The full reductions probe now
passes DIM lowering and stops with `unsupported expression in numeric array
reduction`; array constructors/RESHAPE and elemental MOD need subsequent work.


## Checkpoint 35: RESHAPE and array expressions

Numeric/LOGICAL constructors now materialize as typed temporary arrays in
expressions, including comparisons and reduction arguments. Scalar and fixed
array/section items are concatenated in element order. Shape checks use the
expanded element count; array and section assignments preserve RHS snapshots.
The prior assignment handling of empty constant implied-DOs is retained.

RESHAPE accepts positional or reordered SOURCE=/SHAPE= arguments, a constant
shape constructor and enough source elements. It copies elements in Fortran
array order, preserves element kind, supports zero-sized results and retains
result shape through DIM reductions and bound inquiries. PAD, ORDER and runtime
shape vectors are explicitly unsupported. PURE routines may use RESHAPE.

Elemental MOD/MODULO now scalarize array operands with shape and type checks.
An existing interpreter MOD bug was fixed: real arguments no longer enter the
integer remainder path. Fixture 96 covers the combined reductions workload,
constructor sections and overlapping writes, reshaping, empty results, result
bounds, logical/real arrays and signed integer/real remainders. Focused tests
also exercise invalid shapes/types/keywords, INTEGER kinds and PURE calls.

The full external `xintrinsic_reductions_dim_mask_pending.f90` now passes all
17 checks at O0/O2, increasing the selected sample from 5/18 to 6/18. All 18
gfortran reference runs succeed. Local validation includes 74 Rust unit tests,
96 native fixtures and 11 focused checks (constructors, RESHAPE, DIM and WHERE).

The release compiler was rebuilt and passed all 11 focused checks and 43 batch
wrapper tests. The refreshed release report is `target/ofort-reshape.json`
(6 passes, 12 compile failures). Linux/macOS execution remains untested.


## Checkpoint 36: integer SELECT CASE ranges

Corrected the range grammar from DCOLON to the standard COLON token and added
open lower/upper bounds. Integer bounds fold to constants, omitted limits use
the selector kind's range, and overlapping selectors are diagnosed. Arithmetic
selectors are evaluated into a temporary; CHARACTER ranges are explicitly
rejected in this subset.

Native dispatch previously ignored literal selectors, including those created
by constant propagation and loop unrolling. The backend now selects the matching
case for integer literal selectors instead of always taking CASE DEFAULT.
Fixture 97 covers mixed selector lists, open and closed ranges, PARAMETER and
arithmetic bounds, literal/arithmetic selectors, INTEGER(8), defaults, EXIT and
CYCLE. Focused tests compare interpreter/native O0/O2 with gfortran and check
invalid bounds, overlap, the nonstandard double colon and CHARACTER ranges.

Validation on Windows x64: 74 Rust unit tests, 97 native fixtures, 44 batch
wrapper tests and 5 focused CASE/DO-step tests passed. Both focused suites pass
with debug and rebuilt release compilers. The release compatibility report
`target/ofort-case-ranges.json` has 7/18 passes (11 compile failures), including
`xselect_case_selector_list.f90` at O0/O2. All 18 gfortran reference runs succeed.
Linux/macOS execution remains untested.


## Checkpoint 37: separate PARAMETER statements

The lexer recognizes PARAMETER contextually at the start of a definition
statement, preserving ordinary variables/arrays/procedures named parameter.
The parser records a separate PARAMETER statement as a temporary initialized
declaration marker. Before constant resolution, explicitly declared scalar
names are converted to ordinary typed PARAMETER declarations and removed from
their previous variable declarations. This reuses initializer evaluation,
substitution, module export and assignment-protection handling.

This subset requires preceding scalar type declarations and retains the modern
PARAMETER implementation's supported numeric/CHARACTER types. Implicit typing
and separate array PARAMETER definitions are not yet supported. Duplicate
parameter definitions, dummy arguments, forward/self references and writes to
constants are diagnosed. Fixture 98 covers dependent values, module constants,
CHARACTER padding, real kinds, local shadowing in PURE functions and arrays
named parameter. Focused tests compare interpreter/native O0/O2 with gfortran
and verify the diagnostics.

Validation on Windows x64: 74 Rust unit tests, 98 native fixtures, 45 batch
wrapper tests and both focused tests passed. The focused tests pass with debug
and rebuilt release compilers and compare interpreter/native O0/O2 with gfortran.
The release report `target/ofort-parameter-statements.json` has 8/18 passes
(10 compile failures), with `xparam.f90` passing at O0/O2. All 18 reference runs
succeed. Linux/macOS execution remains untested.

The first release compatibility run reported empty stdout once for the existing
reductions probe at O0 (exit status 0). Six fresh isolated O0 builds/runs all
produced the expected output; an isolated full rerun also passed that probe.
The final report above is that rerun. The initial anomaly was not reproduced
and its cause remains undetermined.


## Checkpoint 38: per-entity CHARACTER lengths

Declarations with `::` now accept entity-specific lengths such as tag*3,
s*(n+1), and PARAMETER tag*(*). Overrides are applied per entity, preserving
the shared length for other names. Non-CHARACTER overrides are diagnosed;
CHARACTER arrays and runtime-dependent lengths retain explicit unsupported
errors.

The declaration rewrite records fixed CHARACTER lengths separately from values.
LEN of a known local/host variable folds in subsequent length expressions and
executable inquiries without reading storage. Local declarations and dummy
arguments hide inherited length information. Fixture 99 covers mixed lengths,
PARAMETER truncation/padding/inference, zero length and dependent declarations.
Focused tests check host association, local shadowing and undefined values in
PURE functions, as well as invalid declarations. Ordinary unused-variable
warnings may still appear for locals used only in folded inquiries.

The external probe also exposed native TRIM of a constant argument producing
empty output. Constant TRIM and LEN_TRIM now fold during parameter rewriting;
fixture 99 checks both on padded/truncated named constants. Nonconstant string
operations continue through their existing execution paths.

Validation on Windows x64: 74 Rust unit tests, 99 native fixtures, 46 batch
wrapper tests and 9 focused CHARACTER tests passed. Both focused suites pass
with debug and rebuilt release compilers and compare interpreter/native O0/O2
with gfortran. The isolated release report
`target/ofort-entity-character-lengths.json` has 9/18 passes (9 compile failures),
including `xcharacter_len_expr.f90` at O0/O2. All 18 reference runs succeed.
Linux/macOS execution remains untested.

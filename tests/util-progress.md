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

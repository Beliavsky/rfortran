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
Dynamic/zero extents, DEALLOCATE/reallocation, multidimensional sections,
array lower bounds other than one, USE-associated multidimensional arrays,
and procedure calls in multidimensional
subscripts remain unsupported. Subscripts must have explicitly declared
scalar integer types of at most 64 bits. Existing initialization diagnostics
refer to the flattened element number. Numeric matrix elements are INTEGER
kinds 1/2/4/8, REAL, or DOUBLE PRECISION.

The full utility program still does not compile. Its next error is
`character(len=*), parameter :: newl = achar(10), ...`. Further blockers include user-defined dotted
operators such as `.in.` and `.exclude.`. The utility library also exercises
features beyond the currently implemented array, character, and I/O subsets.

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

The full utility program still does not compile. Its next error is
`integer(kind=long_int) :: mm(100,100)`. Further blockers include user-defined dotted
operators such as `.in.` and `.exclude.`. The utility library also exercises
features beyond the currently implemented array, character, and I/O subsets.

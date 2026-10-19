# Fortran 2023 additions

The compiler currently implements the following additions. Standard selection is
still a partial compatibility check; see [usage](usage.md#standard-selection).

## Trimmed CHARACTER output

The `AT` edit descriptor outputs a CHARACTER item after removing its trailing
blanks. Leading blanks and nonblank characters are preserved. An all-blank or
zero-length value contributes no characters. Unlike `A`, `AT` cannot have a field
width or precision. It can have a repeat count and occur in nested or unlimited
format groups. It is only valid for output.

```fortran
character(20) :: name = "Alice"
print "('[',at,']')", name       ! [Alice]
```

This works in PRINT, formatted WRITE, internal WRITE, runtime format strings,
array output and nonadvancing output through the shared formatter. Literal
formats and FORMAT statements containing AT are diagnosed in older `-std` modes.
The revision checker does not inspect the values of runtime format variables.

## Logical kinds

`SELECTED_LOGICAL_KIND(BITS)` accepts a scalar INTEGER expression, including a
runtime expression or a keyword argument. It returns a default INTEGER. The
current implementation supports one LOGICAL kind, kind 4 with a 32-bit language
storage model: requests of at most 32 bits return 4, and larger requests return -1.

The result can be used in `LOGICAL(kind=...)` or `LOGICAL(...)` declarations when
it is constant. Scalar and array declarations, function results, and derived-type
components accept the selector. Other LOGICAL kind values are rejected explicitly.

The following new ISO_FORTRAN_ENV constants report current capabilities:

| Constant | Value | Meaning |
|---|---:|---|
| LOGICAL8 | -2 | This size is unsupported; a larger LOGICAL size exists. |
| LOGICAL16 | -2 | This size is unsupported; a larger LOGICAL size exists. |
| LOGICAL32 | 4 | Supported LOGICAL kind. |
| LOGICAL64 | -1 | This size and larger LOGICAL sizes are unsupported. |
| REAL16 | -2 | 16-bit REAL is unsupported; larger REAL sizes exist. |

These additions do not introduce additional storage kinds. `LOGICAL_KINDS`
continues to contain just kind 4. Imports, renaming and re-export through modules
are supported. The new inquiry and constants require Fortran 2023 in revision
checking modes; an ordinary explicit `LOGICAL(4)` declaration remains valid in
earlier modes.

## Delimiter search

`CALL SPLIT(STRING, SET, POS [, BACK])` finds the next delimiter from SET
and updates the scalar INTEGER variable POS. STRING and SET accept scalar
CHARACTER expressions. Keyword arguments and calls from PURE procedures are
supported. POS may also be an integer array element.

For forward searches, start with POS = 0. A match returns its one-based position;
no match returns LEN(STRING) + 1. Subsequent calls start after the preceding
position. With BACK = .TRUE., start with POS = LEN(STRING) + 1; searches start
before POS and return 0 when no delimiter remains. Adjacent, leading, and trailing
delimiters can therefore delimit empty tokens. Empty strings and empty delimiter
sets are accepted. Invalid starting positions produce a runtime error.

```fortran
integer :: pos = 0
call split("one,two", ",", pos)
print *, pos                       ! 4
```

The intrinsic is available in the interpreter and native compiler and requires
`-std=f2023` when a standard is selected. Currently only the default CHARACTER
kind is supported, as elsewhere in the compiler.

## Token arrays and boundaries

The pure generic subroutine TOKENIZE supports both Fortran 2023 interfaces:

```fortran
call tokenize(string, set, tokens [, separator])
call tokenize(string, set, first, last)
```

STRING and SET accept scalar CHARACTER expressions of the default kind and are
evaluated once. Output arguments must be writable, allocatable rank-one arrays.
TOKENS and the optional SEPARATOR require deferred CHARACTER length. FIRST and
LAST require INTEGER kind 1, 2, 4 or 8; their kinds may differ. An index that cannot
be represented in its output kind produces a runtime error.

Every output is allocated with lower bound one, replacing an existing allocation.
TOKENS has one element per token, with element length equal to the longest token;
shorter tokens are blank padded. SEPARATOR has length one and one fewer element
than TOKENS. FIRST and LAST hold the inclusive substring boundaries. Leading,
trailing and consecutive delimiters preserve empty tokens. An empty input yields
one empty token, FIRST = 1 and LAST = 0, and an allocated empty separator array.
An empty delimiter set yields one token containing the entire input.

Keyword arguments, PURE procedures, CHARACTER allocatable output dummies and
assumed-length array input dummies are supported. An unrelated module procedure
named TOKENIZE does not hide the intrinsic. Both interfaces require Fortran 2023
when a standard is selected.

The implementation uses the compiler's existing bounded scalar CHARACTER
representation for input captures and materialized array elements. Statically
known larger capacities are retained; runtime growth beyond an inferred capacity
stops with a diagnostic. The deferred-length array support added here covers
TOKENIZE results and their reads/inquiries; general deferred-length CHARACTER
array allocation and assignment remain incomplete.

## Conditional expressions and arguments

Fortran 2023 parenthesized conditional expressions use this syntax:

```fortran
value = (x > 0 ? x : y > 0 ? y : z)
```

Conditions must be scalar LOGICAL expressions. They are evaluated in order until
one is true; its following expression supplies the result. If none is true, the
final expression supplies the result. No later condition or unselected expression
is evaluated. All alternatives must have the same declared type, kind and rank.
CHARACTER lengths may differ. Nested conditional expressions are supported.

Conditional actual arguments also preserve the identity of a selected variable,
so they can be passed to an INTENT(OUT) or INTENT(INOUT) dummy. Every non-absent
consequent must satisfy the dummy's requirements, even if its condition is a
constant false. Consequents must agree in POINTER and ALLOCATABLE attributes.

```fortran
call update((x > 0 ? x : y), amount=(enabled ? increment : .nil.))
```

Here `.NIL.` means that the corresponding OPTIONAL dummy is absent. It is only
permitted as a consequent of a conditional argument in a procedure reference.
A consequent may contain a conditional expression, but cannot be another
conditional argument containing `.NIL.`.

Choices work in the interpreter and native compiler, including array and derived
values, named and generic calls, constant initializers, automatic array bounds,
loop conditions, and output/constructor implied-DO loops. These features use the
compiler's existing type, array, and procedure support; they do not add general
polymorphism or assumed-rank execution. Selecting a standard requires
`-std=f2023` for either conditional form.

## SIMPLE procedures (US 15)

`SIMPLE` implies `PURE` and also forbids references to host-associated or
USE-associated variables. Pass such data as dummy arguments. Host and module
named constants remain available. All called user procedures and all internal
procedures must themselves be SIMPLE; an ordinary PURE procedure is insufficient.

```fortran
real simple elemental function convert(a) result(b)
integer, intent(in) :: a
b = sqrt(real(a))
end function
```

SIMPLE functions and subroutines work in the interpreter and native compiler,
including ELEMENTAL procedures, module imports, generic calls, and supported
dummy procedure specialization with an explicitly SIMPLE interface. SIMPLE
cannot be combined with PURE or IMPURE. Earlier `-std` modes reject the prefix.
Intrinsic functions and the supported simple intrinsic subroutines (including
SPLIT and TOKENIZE) are allowed, subject to the existing PURE checks.

The attribute does not require compile-time evaluation. General procedure
pointer interfaces, deferred type-bound dispatch, coarrays, COMMON and ENTRY
remain subject to the compiler's existing limitations.

## Multiple subscripts and subscript triplets (US 17)

An INTEGER rank-one array preceded by `@` supplies several consecutive subscripts:

```fortran
print *, a(@[2,3])           ! a(2,3), a scalar element
print *, a(1,@coords,4)      ! ordinary and multiple subscripts can be mixed
print *, a(@lo:hi:2)         ! one section triplet per element of lo/hi
```

This differs from `a(coords)`, an ordinary vector subscript selecting several
values in one dimension. In a multiple triplet, at least one bound or stride
must be a rank-one INTEGER array. Other bounds/strides can be INTEGER scalars,
which are broadcast. Arrays must have the same size. An omitted lower or upper
bound supplies the corresponding array bound; an omitted stride supplies one.
This follows [9.5.3.2 of the Fortran 2023 draft](https://j3-fortran.org/doc/year/24/24-007.pdf),
including when the stride is negative.

The expansion reuses existing array element and section handling, including
assignment destinations, arguments, derived types, and supported ranks up to 15.
INTEGER selector kinds 1, 2, 4 and 8 are supported. Selector expressions are
evaluated once at their execution site, including inside loops, implied-DOs and
lazy conditional branches. Earlier `-std` modes reject this syntax, including
when it appears in an imported module interface.

Current limits: selector sizes must be known statically, or a single unknown
sequence size must be inferable from the parent array's rank. Several sequences
with unknown sizes are diagnosed. Multiple triplets on subscripted array
components currently need explicit bounds. Other existing array-operation
limits still apply. A scalar dummy associated with an array element cannot yet
be used as a DO control variable.

## DO CONCURRENT reductions (US 20)

`REDUCE(operation:variable-list)` supports `+`, `*`, `.AND.`, `.OR.`, `.EQV.`,
`.NEQV.`, `MAX`, `MIN`, `IAND`, `IOR`, and `IEOR`, with multiple clauses and
multiple variables per clause. Variables require an explicit intrinsic type
suitable for the operation. Scalar and fixed-size explicit-shape array reductions support
whole-array, element, and section updates.
The implemented types are INTEGER, REAL, COMPLEX (`+` and `*`), and LOGICAL;
CHARACTER reductions are not supported yet.

```fortran
integer :: i, total = 10, largest = -100
do concurrent (i=1:5) reduce(+:total) reduce(max:largest)
   total = total + i
   largest = max(largest,i)
end do
print *, total, largest    ! 25 5
```

Execution is sequential. Each reduction uses private storage initialized to its
operation's identity and combines the accumulated result with the outside value
after the loop. Empty and entirely masked loops leave the outside value alone.
Index bounds are captured before execution. Masks, multiple index controls,
constant strides, CYCLE (including a construct name), nested loops, and nested
reductions on different variables are supported. Native O0/O2 and interpreter
execution share this lowering. These clauses require Fortran 2023 in strict
standard modes.

Only the standard reduction assignment forms are accepted; references elsewhere,
duplicate variables, incompatible types, prohibited attributes, and transfers
out of the concurrent construct receive source-located errors. POINTER,
ALLOCATABLE, runtime-sized, and assumed-shape/assumed-size reduction objects are not implemented
yet. Named EXIT within a reduction construct is currently rejected, including
when its target is a nested ordinary loop. Other DO CONCURRENT locality
specifiers and parallel execution remain unsupported. Existing DO CONCURRENT
semantic checking is partial; this addition does not claim full validation of
iteration independence or procedure purity.

## Enumeration types (US 21, section 9.2)

Initial support for noninteroperable enumeration types preserves each type's
identity through module association and generic resolution. Values use integer
ordinal storage only after checking assignments, comparisons, and procedure
arguments.

```fortran
enumeration type :: colour
   enumerator :: red, orange, green
end enumeration type colour
type(colour) :: light = orange
print *, int(light)             ! 2
print *, light > red            ! T
```

Definitions accept multiple ENUMERATOR statements, optional double colons,
PUBLIC/PRIVATE module accessibility, and an optional matching name on END
ENUMERATION TYPE. Enumerator names are ordinary named constants in the
containing scope. Assignment requires the same enumeration type on both sides;
there is no implicit conversion to or from INTEGER. The six intrinsic relational
operators compare ordinal positions, starting at one. Arithmetic, logical
operations, enum subscripts/bounds/kinds, and cross-type assignments are rejected.

`colour(i)` constructs a scalar value from a scalar INTEGER ordinal. Constant
out-of-range ordinals are diagnosed during compilation; runtime ordinals are
checked before conversion, including INTEGER(8) values. `INT(A[,KIND])` returns
an ordinal and `HUGE(X)` returns the last enumerator. Keyword arguments to the
supported intrinsics are accepted. Array constructors, constructor implied-DOs,
fixed-size arrays and sections, SELECT CASE (including ranges), scalar procedure
arguments/results, and derived-type component references are supported. Explicitly
typed constructors such as `[colour :: red, green]` and `[colour ::]` retain
their enumeration type for generic dispatch. Their elements can include runtime
values, rank-one arrays, and implied-DOs. Structure constructors accept enumeration
components, including keyword arguments and omitted components with defaults:

```fortran
type signal
   integer :: id=0
   type(colour) :: light=red
end type
type(signal) :: lamp
lamp=signal(light=green)
```

Constructor elements and component values must have the declared enumeration
type; INTEGER ordinals and values of a different enumeration are rejected. Generic
interfaces distinguish enumeration types from INTEGER and from each other.
USE ONLY/renaming, private types with public enumerators, and separate module
compilation retain those distinctions.

`NEXT(A[,STAT])` and `PREVIOUS(A[,STAT])` step to the adjacent enumerator.
At the last/first enumerator respectively, they retain the input value and set
`STAT` to a positive value; successful steps set it to zero. Without `STAT`, a
boundary error terminates execution and identifies the source file and line.
`STAT` must be a writable INTEGER with a decimal exponent range of at least four
(supported INTEGER kinds 2, 4 and 8). Keyword arguments and forwarding an absent
OPTIONAL status dummy through a subroutine are supported. The functions can be used in PURE and SIMPLE
procedures, and valid calls without `STAT` can appear in constant initializers.

These are elemental functions: array inputs produce array results, and any
supplied `STAT` must then be a conformable array. A scalar `A` can broadcast over
an array `STAT`. Fixed-size and runtime-sized arrays through rank 15 are supported,
including automatic locals, assumed-shape dummies, and sections with runtime
bounds or negative strides. `STAT` sections write back to the selected elements
without reading their old values. Shape mismatches are checked at compilation
when possible and otherwise at runtime, with a source file and line. Array calls
with `STAT` use loops, removing the earlier 1,024-element expansion limit.
Calls without `STAT` still honor WHERE masks.
See `tests/programs/xenumeration_steps.f90`, `tests/programs/xenumeration_runtime.f90`,
and `tests/test_enumeration_steps.py`.

An existing list-directed output limitation affects a statement combining
`INT(runtime_array)` and another array as separate output items; use separate
PRINT statements for the enumeration ordinals and statuses for now.

The interpreter and native compiler share these checks and lowering. Strict
revision modes require Fortran 2023. This is the section 9.2 ENUMERATION TYPE
facility, not the distinct interoperable ENUM/BIND(C) facility in section 9.3.

Current limits include direct formatted enumeration I/O,
POINTER/ALLOCATABLE enumeration objects, and some array intrinsics such as
RESHAPE/PACK. List-directed and namelist enumeration I/O are forbidden by the
language; use `INT(value)` to print its ordinal. Polymorphic enumeration objects
are forbidden. Separate local type definitions with the same name in different
procedures currently receive an unsupported-feature diagnostic; separate modules
can define same-named types. Other existing procedure/array limits still apply.
The existing constant folder can confuse main-program named constants with
same-named implicit variables in external procedures; shared enumeration
definitions should be placed in modules to avoid that limitation.

## Try it

```cmd
rf.bat -std=f2023 tests\programs\xfortran2023_basics.f90 --run
rf.bat -std=f2023 tests\programs\xconditional.f90 --run
rf.bat -std=f2023 tests\programs\xtokenize.f90 --run
rf.bat -std=f2023 tests\programs\xsimple_procedures.f90 --run
rf.bat -std=f2023 tests\programs\xmultiple_subscripts.f90 --run
rf.bat -std=f2023 tests\programs\xconcurrent_reduce.f90 --run
rf.bat -std=f2023 tests\programs\xenumeration.f90 --run
rf.bat -std=f2023 tests\programs\xenumeration_steps.f90 --run
echo 4 | rf.bat -std=f2023 tests\programs\xenumeration_runtime.f90 --run
python tests/test_enumerations.py
python tests/test_enumeration_steps.py
python tests/test_concurrent_reduce.py
python tests/test_multiple_subscripts.py
python tests/test_fortran2023.py
python tests/test_simple.py
python tests/test_split.py
python tests/test_tokenize.py
python tests/test_conditionals.py
python tests/test_standards.py
```

The feature tests use the interpreter and native code at O0/O2. Expected logical
kind values describe rfort's capabilities and should not be compared directly
with another compiler that supports a different set of LOGICAL kinds.

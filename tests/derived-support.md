Scalar derived types are lowered into component storage. The supported subset
includes TYPE definitions, TYPE/CLASS object declarations, numeric and logical
scalar components with optional default values, positional constructors,
whole-value assignment, and component access with `%`.

Local procedures can accept derived arguments. Default passed-object bindings
(`procedure :: method` and `procedure :: method => implementation`) use the
first dummy argument. Derived-result functions can be assigned to objects or
passed to INTENT(IN) arguments. Component copies capture their inputs before
writing their outputs. Initialization checks operate on individual components,
including defaults on entry to an INTENT(OUT) procedure.

Array, ALLOCATABLE array and POINTER components are stored as separate
`object%component` variables; `tests/test_derived_array_components.py` covers
them.

CLASS currently uses its declared concrete type. Inheritance, dynamic dispatch,
arrays of derived types with non-scalar components, nested and ALLOCATABLE
scalar components,
keyword constructors, alternate PASS/NOPASS bindings, and an external derived
type ABI are not implemented. Early RETURN in derived-result functions is
diagnosed as unsupported. Type names must be unique within the compilation.

The date tests also cover typed scalar function headers, signed integer and
real MODULO, literal text in explicit output formats, and Iw.m zero padding.
Tests 50-52 and 55 compare valid programs with gfortran; tests 53-54 cover
unset components at compile time and run time.

Module `interface operator(...)` blocks can bind local functions using
`module procedure`. Arithmetic, concatenation, logical, and comparison operator
symbols are accepted, including unary `+`, unary `-`, and `.not.`. Comparison
spellings such as `.eq.` and `==` name the same operator. Overload selection
requires matching scalar operand types and arity; it does not insert numeric
conversions. At least one operand must be a derived type. Operands must have
INTENT(IN). Functions may return derived values or intrinsic scalar values;
typed function headers with RESULT are supported.

Operator visibility follows PUBLIC/PRIVATE declarations and USE association.
Exported operator procedures themselves may remain private. Modules must occur
before modules that USE them, and operator procedure names must be unique in
the compilation. Missing and ambiguous matches are diagnosed. Derived types
have no automatic structural comparison: each comparison, including `/=`,
needs its own interface. Type-bound GENERIC operators, user-defined dotted
operator names, array operands, and separate-compilation operator imports are
not implemented. The existing derived-value limitations above still apply.

Tests 56-57 cover the date operator example, nested derived results,
constructors, assignment aliasing, unary operators, and comparison aliases.

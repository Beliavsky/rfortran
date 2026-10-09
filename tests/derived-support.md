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

CLASS currently uses its declared concrete type. Inheritance, dynamic dispatch,
arrays of derived types, nested/CHARACTER/allocatable/pointer components,
keyword constructors, alternate PASS/NOPASS bindings, and an external derived
type ABI are not implemented. Early RETURN in derived-result functions is
diagnosed as unsupported. Type names must be unique within the compilation.

The date tests also cover typed scalar function headers, signed integer and
real MODULO, literal text in explicit output formats, and Iw.m zero padding.
Tests 50-52 and 55 compare valid programs with gfortran; tests 53-54 cover
unset components at compile time and run time.

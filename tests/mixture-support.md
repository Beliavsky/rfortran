The mixture regression (`47_normal_mixture.f90`) exercises simulation and EM
fitting with a reproducible random stream. `48_array_calls_expressions.f90`
checks optional arguments, array expressions, reductions, loop exits, and
formatted output against gfortran. `49_error_stop.f90` checks error termination.

The new array-call support specializes local subroutines at each call site.
It currently requires whole, rank-one arrays with statically known extents.
Local ALLOCATABLE work arrays are supported when their positive extents can
be determined at the call site (including SIZE-derived local constants).
This is not a general dynamic-allocation or external array-descriptor ABI.
Recursive calls and explicit RETURN in these expanded subroutines remain
unsupported. Optional arguments may be omitted from the end of the positional
argument list; keyword arguments are not implemented.

Whole-array numeric assignment evaluates the right-hand side before writing
the destination. Nested reductions and elemental real math expressions are
supported. Array constructors and numeric array PARAMETER initializers are
supported for rank-one arrays.

Explicit PRINT currently supports flat comma-separated A, I, F, and L
descriptors with repeat counts, quoted literals, and Iw.m integer padding. The descriptor count must equal the item
count; format reversion, nested format groups, and other descriptors are
diagnosed as unsupported. Field widths and precisions are limited to 256.
ERROR STOP currently accepts a character constant and exits with status 1.

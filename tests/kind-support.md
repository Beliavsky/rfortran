REAL declarations accept both `real(dp)` and `real(kind=dp)`, as well as numeric
kind selectors such as `real(kind=8)`. Named selectors can be module parameters
imported through USE.

SELECTED_REAL_KIND and SELECTED_INT_KIND are evaluated for constant integer
arguments, including expressions involving previously declared parameters.
Currently supported calls use positional arguments: P[, R[, RADIX]] for
SELECTED_REAL_KIND and R for SELECTED_INT_KIND. Runtime arguments and keyword
arguments to these intrinsics are not implemented.

Real kind selection advertises binary32 (kind 4, precision 6, range 37) and
binary64 (kind 8, precision 15, range 307). Unsupported precision, range, or
radix requests return negative values. Thus SELECTED_REAL_KIND(32) returns -1;
declaring an unused parameter with that value is valid, but using it as a REAL
kind is rejected. Quadruple-precision arithmetic is not implemented.

Integer kind selection advertises kinds 1, 2, 4, and 8, with decimal ranges
2, 4, 9, and 18 respectively. Larger range requests return -1.

Test 58 and the Windows batch integration suite cover a kind module imported
by a separate main program. Rust unit tests cover selection boundaries,
unavailable kinds, invalid argument types, and both REAL declaration spellings.

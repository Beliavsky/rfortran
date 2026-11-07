# Native DO CONCURRENT parallelism

`--parallel` enables an experimental native worker path. Compilation and
interpretation remain serial by default; `run` (the interpreter) does not accept
this option. Use `--run` to execute the compiled program:

```cmd
set RFORT_NUM_THREADS=4
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel.f90 --run
```

`--parallel-report` explains emitted workers and serial fallbacks. The runtime
uses a persistent pool, defaults to four threads including the calling thread,
and accepts `RFORT_NUM_THREADS` from 1 through 64. The pool size is fixed by its
first parallel job. Ranges shorter than 4096 iterations execute on the calling
thread. Set `RFORT_PARALLEL_TRACE=1` to print the actual thread count for larger
jobs to stderr; unset it for timing runs.

## Supported subset

- Main-program and procedure loops with explicitly declared integer indices
  and nonzero positive or negative strides, including runtime stride values.
  Captured dummy arguments require explicit INTENT.
- INTEGER, REAL, DOUBLE PRECISION, or LOGICAL arrays: fixed-size
  arrays with constant bounds, and ALLOCATABLE arrays with runtime extents
  and arbitrary lower bounds. Supported assumed-shape dummy arrays are also
  accepted. Constant extents may use named constants.
- Writes such as `a(i)`, `a(i+1)`, `a(i,1)`, and `a(i+offset,j,slot)`.
  Every concurrent index must occur exactly once in the destination tuple;
  additional subscripts and additive offsets must be loop-invariant integer
  expressions. Every access to a written array must use the same subscript
  expressions. Read-only arrays may use other scalar subscripts.
- Scalar and budgeted fixed/runtime-sized array `LOCAL` and `LOCAL_INIT`, shared scalar
  inputs, masks, and IF statements.
- Scalar and fixed-size or runtime-sized array INTEGER/REAL/DOUBLE PRECISION `REDUCE(+:...)`, `REDUCE(*:...)`,
  `REDUCE(MIN:...)`, and `REDUCE(MAX:...)`; scalar and fixed-size or runtime-sized array LOGICAL `.AND.`, `.OR.`,
  `.EQV.`, and `.NEQV.` reductions. Multiple reductions and reduction-only
  loops are supported.
- Arithmetic, comparisons, and audited scalar intrinsics, both directly and in
  proved PURE/SIMPLE callees: SQRT, ABS, INT, REAL, DBLE, MIN/MAX;
  MOD/MODULO, SIGN, DIM, DPROD; NINT, FLOOR/CEILING, AINT/ANINT;
  SIN/COS/TAN, ASIN/ACOS/ATAN/ATAN2 (including two-argument ATAN),
  SINH/COSH/TANH and their inverses; EXP, LOG/LOG10, HYPOT;
  ERF/ERFC, ERFC_SCALED, GAMMA, and scalar BESSEL_J0/J1/JN and
  BESSEL_Y0/Y1/YN for REAL kinds 4/8; scalar MERGE for supported INTEGER,
  REAL and LOGICAL values;
  HUGE/TINY/EPSILON, DIGITS, RADIX, PRECISION, RANGE, MINEXPONENT,
  MAXEXPONENT, BIT_SIZE, and KIND. Real model functions SCALE, FRACTION,
  EXPONENT, SET_EXPONENT, NEAREST, SPACING, and RRSPACING work for REAL kinds
  4 and 8, including calls from proved PURE/SIMPLE procedures.
  Positional and keyword arguments work.
  Integer bit intrinsics also work for kinds 1, 2, 4, and 8: BTEST, IAND,
  IOR, IEOR, NOT, IBSET, IBCLR, IBITS, ISHFT, SHIFTL/SHIFTR/SHIFTA,
  DSHIFTL/DSHIFTR, MERGE_BITS, POPCNT/POPPAR, LEADZ/TRAILZ, ISHFTC,
  MASKL/MASKR, and BGE/BGT/BLE/BLT.
  Dynamic positions, field lengths, and shifts retain their range checks,
  including with `--nocheck`. ISHFTC checks SIZE before rotating and accepts
  shifts from -SIZE through SIZE. MASKL/MASKR support optional KIND, and
  bit-sequence comparisons zero-extend the shorter operand when kinds differ.
  Procedures shadowing these names do not qualify as intrinsic calls.

Special-function calls use the platform C math library, whose
[GNU implementation documents these operations as MT-Safe](https://sourceware.org/glibc/manual/2.41/html_node/Special-Functions.html).
The Bessel wrappers and scaled-error-function helper keep all working values
local to each invocation. ERFC_SCALED evaluates X once and switches to a scaled
asymptotic tail before EXP(X*X) can overflow. Bessel JN/YN orders are passed in
double precision so conversion does not round valid C-int orders through REAL(4);
the result retains X's kind. The three-argument transformational Bessel forms
remain unsupported.

```cmd
python tests\test_concurrent_parallel_special.py
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_special.f90 --run
```

The source proof and native callee recheck share the scalar eligibility catalog.
FLOOR/CEILING range errors still terminate the program with their normal diagnostic.
Other pure intrinsics require a separate runtime audit; in particular LOG_GAMMA
is excluded because some C library implementations use [shared sign state](https://man.openbsd.org/lgamma.3).
Run `python tests/test_concurrent_parallel_intrinsics.py` for direct-loop,
PURE-function, SIMPLE-subroutine, fallback, and rounding-error coverage. Try:

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_intrinsics.f90 --run
```

The bit-operation tests cover all supported integer kinds, sign bits, empty and
full-width fields, zero/full-width shifts, reordered keywords, and invalid
runtime controls in direct loops and PURE/SIMPLE callees:

```cmd
python tests\test_concurrent_parallel_bits.py
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_bits.f90 --run
```

Rotation, mask and unsigned-comparison tests also check optional arguments,
constant initialization, mixed-kind comparisons and single evaluation of actuals:

```cmd
python tests\test_concurrent_parallel_bit_extensions.py
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_bit_extensions.f90 --run
```

Real model operations use an audited runtime helper with invocation-local
arithmetic and bit patterns. The native callee check accepts only the selected
operation tags and REAL kinds 4/8, rather than every opcode sharing that helper.
NEAREST retains its zero-direction error, including with `--nocheck`.
Tests cover both kinds, mixed-kind direction arguments, integer exponent
arguments, native real/integer results, subnormals, signed zero, and propagation
of infinities/NaNs through SCALE and NEAREST:

```cmd
python tests\test_concurrent_parallel_real_model.py
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_real_model.f90 --run
```

Loop bounds and strides are evaluated once before dispatch. Workers receive
explicit captures; scalar captures are private copies and supported arrays share their existing
storage. Initialization and bounds checks remain enabled unless `--nocheck`
is requested. A job completes before execution continues after the loop.

Reduction metadata transports storage addresses rather than reading Fortran
values. Initialization checking therefore does not scan the worker-result
buffers on every iteration. For example, eight reduction elements reserve
512 result slots (space for up to 64 workers), so scanning the full result mask once per
iteration was substantially more work than the reduction itself.
Actual value reads and evaluated view extents
retain their checks. In particular, uninitialized reduction inputs and invalid
subscripts are still diagnosed. Regression coverage is in
`tests/test_concurrent_parallel_reduction_checks.py`.

For validated performance-sensitive code, `--nocheck` explicitly disables
initialization and bounds checks. Compare both configurations with the
[parallel benchmark harness](../benchmarks/parallel/README.md); `-O2` by itself
preserves checking.

Other constructs use the existing serial lowering. In particular this stage
does not parallelize unproved procedure calls, allocation of shared storage,
I/O, outer constructs containing loops, named constructs, derived
types, CHARACTER, or pointer array storage. Arrays with
TARGET and other unsupported attributes also fall back to serial execution.
This conservative selection is not a complete checker of all Fortran
DO CONCURRENT restrictions. Serial fallback does not establish that an otherwise
invalid DO CONCURRENT program is conforming.

## Multiple indices and multidimensional arrays

Multiple controls are flattened to one work range, with the first written
array's first subscript varying fastest for column-major locality. The mapping
is rank-independent; tests cover ranks two, three, and fifteen. Fixed arrays,
runtime-sized allocatables, and assumed-shape procedure arguments use their
existing storage mapping and checks. Masks and scalar reductions work across
the whole iteration space. An empty dimension makes the entire space empty.
The mapping reconstructs each index from its starting value and signed stride.
An endpoint need not be reached exactly; a stride pointing away from the end
produces an empty range. Zero strides are diagnosed even with `--nocheck`.

The current mapping requires the normalized inclusive bound span, stride
magnitude, and total iteration count to fit signed 64-bit arithmetic. It
checks these limits before dispatch, even with `--nocheck`. The bound-span
check is conservative: extremely wide ranges may be rejected even when a
large stride would make the iteration count fit. Empty dimensions skip span
and product calculations.

Offsets and fixed subscripts may use shared read-only integer scalars or
named constants, together with addition, subtraction, and unary negation.
LOCAL, LOCAL_INIT, and REDUCE variables cannot supply invariant offsets or
fixed subscripts. Index scaling, mixed indices in one subscript, and changes
of subscript expression between accesses to the same written array retain
serial execution. This proof is deliberately more restrictive than the full
language.

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_matrix.f90 --run
```

A strided example with a fixed column, an offset, and a scalar reduction:

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_stride.f90 --run
```

## Private LOCAL and LOCAL_INIT arrays

Numeric (including COMPLEX kinds 4/8) and LOGICAL arrays with fixed or runtime
bounds can appear in `LOCAL` and `LOCAL_INIT`. Each worker owns its data and
initialization masks. `LOCAL` starts undefined on every iteration; a definition
in an earlier iteration does not make a later read valid. `LOCAL_INIT` restores
the loop-entry snapshot on every iteration, and leaves the outside array unchanged.
The snapshot is taken again for each execution of the construct.

Whole-array assignments, element/section accesses, audited array intrinsics,
and supported PURE/SIMPLE calls use the existing worker lowering. Multidimensional
arrays, nondefault bounds, masks, multiple concurrent indices, and reductions
can be combined with these private workspaces. Bounds and initialization checks
remain active unless disabled with `--nocheck`; the storage remains private in
either mode.

Runtime-sized workspaces inherit the original array's established bounds and
extents, even when a specification-bound variable changes after procedure entry.
Descriptor lengths and the aggregate budget are evaluated before dispatch.
Private arrays retain their shape within the loop; array assignments use section
semantics rather than reallocating worker storage. Caller-owned temporary
descriptors are released after the construct, and workers release their copies
after completing their chunks. Allocation failures retain runtime diagnostics.

The budget is 4096 components summed across all `LOCAL`/`LOCAL_INIT` arrays in
one construct; each COMPLEX element counts as two components. This budget is
separate from the array-reduction budget. Larger and zero-sized private arrays
use serial execution; runtime extents select that path before worker dispatch.
ALLOCATABLE, POINTER, derived-type and
CHARACTER locality objects retain their existing restrictions.

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_local_arrays.f90 --run
python tests\test_concurrent_parallel_local_arrays.py
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_runtime_local.f90 --run
python tests\test_concurrent_parallel_runtime_local.py
```

## Scalar reductions

Each worker starts with the operation's identity and writes its partial result
into a separate slot. After the workers finish, the calling thread combines
active workers' results in worker-index order and incorporates the outside
variable's initial value once. Empty ranges and masks selecting no iterations
leave that outside value unchanged. This also works when only some workers have
active iterations. Floating-point results may differ from serial execution or
from another thread count because the additions and multiplications are grouped
differently.

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_reduce.f90 --run
```

## Array reductions

Fixed-size and runtime-sized INTEGER, REAL, DOUBLE PRECISION, and LOGICAL arrays support the
same reduction operators as scalars. Tests cover ranks one, two, three, and
fifteen, nondefault bounds, whole-array and element/section updates, section
inputs, and repeated procedure calls. Each worker receives a heap-allocated
copy initialized to the reduction identity. Its result goes into a separate
slice of an allocatable result buffer. The caller folds active slices in
worker order, then incorporates the original array once. Worker copies and
result buffers are freed after use. Empty ranges and fully masked loops leave
the original array unchanged.

The conservative budget is 4096 storage components summed across all reduction
arrays in a loop; each COMPLEX element counts as two real components.
Result buffers reserve 64 slots, matching the pool's maximum size;
worker copies and result data/initialization masks use at most 6 MiB of payload.
Ordinary caller storage, expression temporaries, and allocator overhead are
additional. Larger reductions and zero-sized reduction arrays use the serial
fallback. For runtime-sized arrays this budget is checked before allocating
worker-result buffers. The ordinary serial accumulator still needs one copy of
the reduction array. ALLOCATABLE arrays must already be allocated; supported
assumed-shape and explicit-shape dummy arguments also work. Reallocation or
changing shapes between calls is supported. POINTER reduction objects remain
unsupported.

Fixed-size and runtime-sized section temporaries are worker-private, including
their heap descriptors and initialization masks. Runtime storage is allocated
inside the iteration after its section bounds are known and freed by the normal
lexical cleanup. Section lengths may change between iterations; empty sections,
negative strides, masks, and repeated calls with changing array shapes are supported.
Temporary sizes use the ordinary allocation-size overflow and allocation-failure
checks. The 4096-element reduction budget does not include expression temporaries.
Scratch storage without a proven worker-local allocation retains serial fallback,
reported by `--parallel-report`.

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_array_reduce.f90 --run
```

Unsupported captures and unproved procedure calls inside loops retain serial fallback.

A runtime-sized rank-two reduction with nondefault bounds:

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_runtime_reduce.f90 --run
```

A runtime-sized input section and reduction section:

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_sections.f90 --run
```

## COMPLEX values

COMPLEX kinds 4 and 8 are supported for scalar captures, independent writes to
fixed-size and runtime-sized arrays, scalar and budgeted array LOCAL/LOCAL_INIT, and checked
PURE/SIMPLE functions and subroutines. Supported expressions include complex
literals, assignment, addition, subtraction, multiplication, division, CMPLX,
REAL, AIMAG, CONJG and ABS. Named kind parameters, multidimensional arrays,
strided read-only arguments, complex scalar/array results and private fixed or
allocatable workspaces use the existing specialization and storage checks.

The compiler lowers each complex value to real and imaginary components before
native outlining; the backend still proves ordinary real storage and helper
calls. Each component retains its own initialization state. LOCAL clears both
states on every iteration without reading the old value; LOCAL_INIT copies both
components and leaves the outside variable unchanged. Numeric initialization
and bounds checks apply unless disabled with --nocheck.

The existing independence rules apply to the whole source complex element,
before component lowering. Reading another element of a written array,
unsupported aliasing, POINTER/TARGET storage and unproved procedure calls keep
serial fallback.

COMPLEX scalar and fixed-size/runtime-sized array reductions support
`REDUCE(+:...)` and `REDUCE(*:...)`. Workers start with complex zero or one,
publish both components of each partial, and the caller folds active partials
in worker order using complex arithmetic. Multiplication combines each real
and imaginary pair together. The original accumulator is included once;
empty loops and masks selecting no iterations preserve it. Array reductions
use the storage budget above, with serial fallback for empty or over-budget
arrays. Floating-point rounding can depend on the thread count.

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_complex.f90 --run
python tests\test_concurrent_parallel_complex.py
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_complex_reduce.f90 --run
python tests\test_concurrent_parallel_complex_reduce.py
```

## PURE and SIMPLE functions

Workers can use user-defined PURE and SIMPLE functions with explicitly declared
numeric (including COMPLEX) or LOGICAL scalar, fixed-size, automatic, or allocatable array results. Scalar dummy arguments
require `INTENT(IN)` or `VALUE`; a VALUE dummy may be modified locally.
Numeric and LOGICAL array dummy arguments require `INTENT(IN)`. Fixed-size and
runtime-sized actual arrays and regular strided sections are supported, including
multidimensional inputs, empty sections, and section lengths that change between
iterations. Tests cover ranks one, two, and fifteen. Assumed-shape dummies currently
require default lower bounds; actual arrays may have nondefault lower bounds.
Scalar expressions, keyword arguments, local loops,
branches, early returns, and acyclic chains of supported functions are accepted.
Named constants, including module constants and named numeric kinds, can be used.
INTEGER, REAL and LOGICAL host/module variables can also be read when the loop and
its proven call chain leave them unchanged. Scalar captures are copied into
worker storage at dispatch; array data is shared read-only. This includes
fixed-size and allocatable module arrays, with bounds and allocation status
preserved, and host variables promoted to module storage by the compiler.

Supported PURE callees receive these inputs through private helper copies with
explicit read-only arguments; the original procedure interfaces stay unchanged.
Helpers are inlined before worker generation so a guarded read retains its
original initialization checks inside the branch. Changes to shared values and
array allocation between loops are observed by the next dispatch. Recursive,
unavailable, ELEMENTAL-with-captures, COMPLEX-valued captures in callees, BIND(C),
and unproved call chains retain their original calls and conservative serial fallback.

Numeric (including COMPLEX) and LOGICAL `OPTIONAL, INTENT(IN)` arguments are
supported in proved PURE/SIMPLE callees. Scalars and arrays may be supplied,
omitted, passed by keyword, or forwarded through an acyclic optional call chain.
Existing presence specialization selects a present/absent procedure variant
before worker generation; absent dummies are removed and `PRESENT` is folded.
Read-only optional whole scalar variables are borrowed during scoped expansion, so
testing presence or bypassing a guarded read does not fetch an undefined value.
Array bounds and initialization checks remain active. Allocatable actual arrays
must be allocated; runtime absence inferred from unallocated actuals is not
implemented by this presence specialization. Optional POINTER/ALLOCATABLE dummies
and possible writable aliasing retain serial fallback.

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_optional.f90 --run
python tests\test_concurrent_parallel_optional.py
```

Writes to host/module storage still use serial execution. A loop with a writable
dummy and read-only host/module inputs also falls back, since the actual dummy
could alias that shared storage. POINTER and TARGET inputs remain excluded.
Run `python tests/test_concurrent_parallel_captures.py` for these cases.

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_captures.f90 --run
```

The source analysis checks the whole call chain. A second check examines the
native functions after specialization and lowering: scalar storage must be local
to the call, and generated helpers must also be proved thread-safe. The existing
native scalar calling convention supplies per-call stack storage for arguments,
results, and local initialization flags. Runtime initialization checks remain
active. Scalar-only functions without captures use native calls. Array-taking functions reuse the
compiler's existing call-site specialization before the loop is outlined. Their
generated scalar temporaries, section buffers, descriptors, and initialization
masks are private to workers. Array inputs must be read-only throughout the loop;
passing an array that the loop also writes retains serial fallback.

Functions may declare fixed-size, automatic, or allocatable numeric/LOGICAL local arrays,
including multidimensional workspaces such as `real :: work(size(a),2)`.
Assignments to local arrays and regular local sections are supported. The
function and its supported callers are specialized at the call site; workspace
buffers, descriptors, and initialization masks stay private to the worker.
Automatic storage is allocated on entry and released at the lexical exit,
including early RETURN. Sizes may change between invocations, and empty
workspaces are supported. Initialization masks are reset for each invocation;
initializing a workspace in one call does not define it in the next.
VALUE scalar arguments use private copies even when the function modifies them.
Allocatable local workspaces support allocation through assignment, explicit
ALLOCATE/DEALLOCATE, changing shapes, empty arrays, explicit lower bounds,
and positional or keyword ALLOCATED inquiries. Their descriptors, data, and
initialization masks are private to each invocation. Local allocations are
released on normal completion and early RETURN; repeated calls do not inherit
allocated state or initialization flags. Tests cover ranks one, two, three,
and fifteen. Pointer, target, and saved workspace storage remains excluded.
Functions with scalar-only arguments and results retain their native definitions as well
as the call-site templates.
The ordinary allocation-size, allocation-failure, initialization, and subscript
checks remain active unless `--nocheck` is requested.

Array-valued functions use private result descriptors, data, and initialization
masks at each invocation. Fixed and runtime result extents, multidimensional
results, explicit lower bounds, empty results, nested function calls, and early
RETURN are supported. Results can be consumed by another supported function or
array reduction, assigned to a local workspace, or assigned to disjoint shared
array sections such as `y(:,i)=values(n,i)`. For section output, each concurrent
index must select a separate axis with a scalar subscript; section bounds and
strides on the other axes must be invariant. Overlapping sections such as
`y(i:i+1)=pair(i)` retain serial fallback. Result storage is released at its
lexical exit, and initialization checks do not inherit definitions from earlier calls.

An allocatable array result can be allocated by whole-array assignment or by
`ALLOCATE` of the result itself. `DEALLOCATE` and `ALLOCATED` inquiries of that
result are also supported, including `ALLOCATED(array=r)`. Result shapes may change within and between calls,
including empty arrays; explicit lower bounds and multidimensional results are
supported. Nested calls and self-referential assignment use the existing
descriptor and assignment paths, with private storage and initialization masks.
Unallocated results, undefined elements, and out-of-bounds accesses retain
runtime checks. Explicit allocation supports a single owned local array or result
target with `STAT=`, `SOURCE=`, and `MOLD=`. STAT must name a local scalar INTEGER
variable, rather than a dummy argument, named constant, or shared variable.
Success sets it to zero; allocating an already allocated array or deallocating
an unallocated array sets it nonzero and preserves the array's state. SOURCE
initializes the requested allocation and preserves explicit bounds; a shape
mismatch is diagnosed instead of resizing the target. MOLD supplies shape/type
information without defining elements or reading the model's values.
Allocation-state inquiries in SOURCE are captured before allocation.
ERRMSG and allocation of multiple targets in one statement retain serial fallback.
STAT also reports extent/product overflow and failure to allocate the INTEGER/REAL/LOGICAL
target or its initialization mask. A failed target remains unallocated;
partial storage is freed, and SOURCE copying is skipped. Compiler-generated
expression temporaries and CHARACTER/derived-type allocations retain their
existing allocation-failure behavior.

Array-taking functions may use SUM, PRODUCT, MINVAL, MAXVAL, COUNT, ANY, ALL,
IALL, IANY, IPARITY, PARITY, NORM2, MINLOC, MAXLOC, FINDLOC,
DOT_PRODUCT, SIZE, LBOUND, and UBOUND, as well as the scalar intrinsics listed
above. This includes direct loop expressions and proved PURE/SIMPLE callees,
read-only runtime-sized and strided inputs, and supported constant/runtime DIM
results. Bit reductions support INTEGER kinds 1/2/4/8; PARITY uses LOGICAL.
NORM2 supports REAL kinds 4/8 and accumulates with scaled HYPOT calls rather than
squaring the inputs, avoiding unnecessary overflow and underflow. Empty norms
are zero. Location searches retain MASK, BACK, KIND, first/last tie-breaking,
one-based result positions and zero for no match. Numeric searches and LOGICAL
FINDLOC are covered; CHARACTER storage retains the restrictions below.
All generated reduction/search temporaries belong to the worker invocation.
Run `python tests/test_concurrent_parallel_array_intrinsics.py` or try
`rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_array_intrinsics.f90 --run`.
 A user procedure shadowing an intrinsic does not qualify as an intrinsic
call. Lowered storage and runtime helpers still have to pass the native checks.

Definitions must be available in the compilation group. Recursive call chains,
POINTER results, OPTIONAL/POINTER/TARGET/ALLOCATABLE
dummies, CHARACTER storage, saved storage, I/O, allocation of shared or aliased storage,
unproved subroutine calls, POINTER/TARGET/SAVE storage, and unproved
callees retain serial fallback. Recursive workspace functions keep their
existing serial calling path.
Purity alone does not bypass the storage and runtime-helper checks. Use
`--parallel-report` to see the reason for a fallback.

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_functions.f90 --run
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_array_functions.f90 --run
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_workspace.f90 --run
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_array_results.f90 --run
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_allocatable_results.f90 --run
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_allocatable_workspace.f90 --run
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_allocation_options.f90 --run
```

## PURE and SIMPLE subroutine calls

Concurrent bodies may call available PURE/SIMPLE subroutines with explicit
INTENT on INTEGER/REAL/LOGICAL dummies and matching actual/dummy ranks. Scalar expressions and read-only arrays
are accepted as inputs. OUT/INOUT actuals must be supported LOCAL/LOCAL_INIT
variables or array elements/regular sections whose destination tuple contains
each concurrent index exactly once. For example:

```fortran
do concurrent(i=1:n)
   call transform(x(i),y(i))
end do
```

A fixed column such as `a(:,i)` is disjoint across iterations and can be passed
to an array output dummy. All writes to a given array must use the same tuple;
reads of written storage must stay within the same proven destination tuple.
Whole-array reads of a written array retain serial fallback. If a writable base
variable occurs in another actual argument of the same call, the compiler
conservatively uses serial execution, including for different views of one array.

Sequence association from a scalar array element to an array dummy retains
serial execution because the callee can access more than the named element.

The source proof follows acyclic chains of supported subroutines and functions.
Existing scoped call expansion binds actuals before the call, gives locals and
workspaces private storage, undefines INTENT(OUT) actuals on entry, and preserves
early RETURN and cleanup. Initialization checks cover INOUT reads and OUT paths
that leave elements undefined. Fixed/automatic/allocatable workspaces use the
same limits and allocation checks described for functions above.

OPTIONAL numeric and LOGICAL `INTENT(OUT)` and `INTENT(INOUT)` arguments also
use presence specialization. Supplied outputs must satisfy the same disjoint
scalar-element or array-section proof as required outputs. Omitted outputs are
removed before scoped expansion, including when forwarded through PURE/SIMPLE
call chains. Keyword arguments and runtime-sized multidimensional sections work.
`INTENT(OUT)` undefines the actual on entry even when the callee leaves it
unwritten; `INTENT(INOUT)` retains initialization checks on reads. Bounds checks
and conservative aliasing fallbacks remain active.

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_optional_outputs.f90 --run
python tests\test_concurrent_parallel_optional_outputs.py
```

POINTER, TARGET, ALLOCATABLE dummies, derived/CHARACTER arguments, saved local storage,
writes to host/module storage, procedure dummies, and recursive or unavailable
callees retain serial fallback. Read-only host/module inputs follow the rules above.
REDUCE variables retain the existing requirement that they occur in reduction
assignments; passing them through a subroutine is not yet accepted.

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_subroutines.f90 --run
python tests\test_concurrent_parallel_subroutines.py
```

## Runtime-sized arrays

Allocatable arrays can be allocated before a concurrent loop and
read or written by its workers. The capture record references the live data
and initialization-mask cells; bounds, extents, and allocation status are
copied into each worker. Allocation and deallocation must stay outside the
loop, and the synchronous join finishes before the caller can change storage.
Reallocation between successive loops is supported.

Bounds and initialization checks remain active, including with negative lower
bounds. Empty arrays and empty iteration ranges require no worker access.
The same independence rules apply: a written array may only be read or written
at the current iteration's subscript. Read-only arrays may use other subscripts.

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_allocatable.f90 --run
```

## Procedure arguments

Loops inside procedures can use assumed-shape arrays with explicit
`INTENT(IN)`, `INTENT(OUT)`, or `INTENT(INOUT)`. Fixed-size and allocatable actual
arrays are supported, together with regular strided sections handled by the
existing argument specialization. The workers use that established argument
mapping and its bounds and initialization checks. Supported LOCAL/LOCAL_INIT and
REDUCE may be combined with these array arguments.

POINTER, TARGET, OPTIONAL, missing INTENT, writes to captured host/module state,
possible aliasing of writable dummies with that state, and saved local storage
retain conservative serial fallbacks. These checks are not a
complete validator of Fortran argument aliasing rules: callers must still obey
those rules, particularly when one dummy argument is modified. Calls to supported
scalar PURE/SIMPLE functions inside the concurrent body use the checks above;
supported PURE/SIMPLE subroutines use scoped call expansion as described above. Other procedure calls retain serial execution.

```cmd
rf.bat --parallel --parallel-report tests\programs\xconcurrent_parallel_dummy.f90 --run
```

## Implementation and testing

The front end proves the supported access pattern, then the native backend
outlines the loop into a callback with an explicit capture record. The runtime
uses Windows threads or POSIX threads, without an OpenMP dependency. Pool
workers wait on events on Windows and use short sleeps on POSIX platforms;
further tuning and broader capture support are future work. Small or memory-bound kernels may not benefit from threads.

```cmd
python tests\test_concurrent_parallel.py
cargo test --bin rfort parallel_runtime_tests
```

Tests compare results with one, two, and four threads, check repeated dispatch,
masks, local storage, scalar reductions, allocatable arrays, serial fallback,
and runtime diagnostics. The runtime test
also checks simultaneous callers, negative iteration bounds, and exact-once
coverage. Release smoke tests require a two-thread run on each release platform.
Local verification was performed on Windows x86-64; Linux and Apple Silicon
execution is left to those CI runners.

## Initial Windows timing

The [parallel benchmark suite](../benchmarks/parallel/README.md) compares serial
builds with one-, two-, and four-thread parallel execution across seven kernels
and several sizes, with correctness checks and separate worker-dispatch probes.
Use it for repeatable measurements beyond the illustrative timing below.

On an Intel Core i7-1185G7 (four cores, eight logical processors),
[`xconcurrent_parallel_math.f90`](../tests/programs/xconcurrent_parallel_math.f90)
produced the following whole-program wall times. The native code used `-O2`
with runtime checks enabled. Each result is the median of five runs after one
warmup, with the order of the configurations rotated between rounds and no
compiler build running. Every run printed the same result.

| Configuration | Median seconds | Serial time / time |
|---|---:|---:|
| Serial compilation | 0.432 | 1.00 |
| Parallel compilation, one thread | 0.404 | 1.07 |
| Parallel compilation, two threads | 0.279 | 1.55 |
| Parallel compilation, four threads | 0.248 | 1.74 |

This is one illustrative math kernel, not a general speedup claim. The single-
thread difference includes generated-code differences and timing noise. The
measurement includes process startup and pool creation. More workloads and
platforms need measurement before choosing automatic parallel execution.

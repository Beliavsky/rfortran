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
- Scalar `LOCAL` and `LOCAL_INIT`, shared scalar inputs, masks, and IF statements.
- Scalar and fixed-size or runtime-sized array INTEGER/REAL/DOUBLE PRECISION `REDUCE(+:...)`, `REDUCE(*:...)`,
  `REDUCE(MIN:...)`, and `REDUCE(MAX:...)`; scalar and fixed-size or runtime-sized array LOGICAL `.AND.`, `.OR.`,
  `.EQV.`, and `.NEQV.` reductions. Multiple reductions and reduction-only
  loops are supported.
- Arithmetic, comparisons, and a small set of scalar intrinsics: SQRT, ABS, INT,
  REAL, DBLE, SIN, COS, EXP, LOG, MIN, and MAX. Procedures shadowing these names
  do not qualify as intrinsic calls.

Loop bounds and strides are evaluated once before dispatch. Workers receive
explicit captures; scalar captures are private copies and supported arrays share their existing
storage. Initialization and bounds checks remain enabled unless `--nocheck`
is requested. A job completes before execution continues after the loop.

Other constructs use the existing serial lowering. In particular this stage
does not parallelize subroutine calls, unproved function calls, allocation,
I/O, outer constructs containing loops, named constructs, derived
types, CHARACTER, complex arrays, or pointer array storage. Arrays with
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

The conservative budget is 4096 elements summed across all reduction arrays
in a loop. Result buffers reserve 64 slots, matching the pool's maximum size;
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

## PURE and SIMPLE functions

Workers can use user-defined PURE and SIMPLE functions with explicitly declared
numeric or LOGICAL scalar, fixed-size, automatic, or allocatable array results. Scalar dummy arguments
require `INTENT(IN)` or `VALUE`; a VALUE dummy may be modified locally.
Numeric and LOGICAL array dummy arguments require `INTENT(IN)`. Fixed-size and
runtime-sized actual arrays and regular strided sections are supported, including
multidimensional inputs, empty sections, and section lengths that change between
iterations. Tests cover ranks one, two, and fifteen. Assumed-shape dummies currently
require default lower bounds; actual arrays may have nondefault lower bounds.
Scalar expressions, keyword arguments, local loops,
branches, early returns, and acyclic chains of supported functions are accepted.
Named constants, including module constants and named numeric kinds, can be used;
mutable module/host storage is excluded from this initial subset.

The source analysis checks the whole call chain. A second check examines the
native functions after specialization and lowering: scalar storage must be local
to the call, and generated helpers must also be proved thread-safe. The existing
native scalar calling convention supplies per-call stack storage for arguments,
results, and local initialization flags. Runtime initialization checks remain
active. Scalar-only functions use native calls. Array-taking functions reuse the
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
runtime checks. Explicit allocation currently requires a single owned local array or result target
without control specifiers such as `STAT`, `ERRMSG`, `SOURCE`, or `MOLD`.

Array-taking functions may use SUM, PRODUCT, MINVAL, MAXVAL, COUNT, ANY, ALL,
DOT_PRODUCT, SIZE, LBOUND, and UBOUND, as well as the scalar intrinsics listed
above. A user procedure shadowing an intrinsic does not qualify as an intrinsic
call. Lowered storage and runtime helpers still have to pass the native checks.

Definitions must be available in the compilation group. Recursive call chains,
POINTER results, OPTIONAL/POINTER/TARGET/ALLOCATABLE
dummies, CHARACTER/COMPLEX storage, saved storage, I/O, allocation of shared or aliased storage,
subroutine calls, POINTER/TARGET/SAVE storage, and unproved
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
mapping and its bounds and initialization checks. Scalar LOCAL/LOCAL_INIT and
REDUCE may be combined with these array arguments.

POINTER, TARGET, OPTIONAL, missing INTENT, captured host/module state, and saved
local storage retain conservative serial fallbacks. These checks are not a
complete validator of Fortran argument aliasing rules: callers must still obey
those rules, particularly when one dummy argument is modified. Calls to supported
scalar PURE/SIMPLE functions inside the concurrent body use the checks above;
other procedure calls retain serial execution.

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

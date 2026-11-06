# Parallelism options for rfort: coarrays, DO CONCURRENT, OpenMP

Planning notes, November 2026. Serial DO CONCURRENT locality and an opt-in native parallel subset are implemented; broader parallelism remains proposed.

## Current state

rfort executes serially by default. See [native DO CONCURRENT parallelism](parallel-concurrent.md) for the experimental `--parallel` option.

- Coarrays are unsupported (`docs/feature-status.md`). Nothing in `src/`
  parses codimensions, image selectors or SYNC statements.
- OpenMP and OpenACC directives are treated as comments, and `_OPENMP` is not
  defined.
- DO CONCURRENT has a serial fallback. Its controls, mask, locality clauses and body
  remain together in the parser representation until semantic lowering.
  LOCAL, LOCAL_INIT, SHARED, DEFAULT(NONE), and REDUCE are supported with the
  storage limits documented in [language support](language-support.md#do-loops).
  An opt-in native path outlines supported main-program and procedure loops and dispatches
  them to a reusable pool, including multidimensional allocatable arrays and scalar/array numeric and logical reductions; unsupported loops remain serial.

## Coarrays in three stages

### Stage 1: a single image (like gfortran `-fcoarray=single`)

The program runs as one image, so each coarray feature has a trivial meaning:

- `num_images()` is 1 and `this_image()` is 1. `x[1]` is `x`, and any other
  image index is a run-time error.
- SYNC ALL, SYNC IMAGES, SYNC MEMORY and CRITICAL do nothing. LOCK/UNLOCK and
  EVENT POST/WAIT need only simple bookkeeping; LOCK must still detect a lock
  the image already holds.
- `co_sum`, `co_min`, `co_max`, `co_broadcast` and `co_reduce` leave their
  argument unchanged. Atomic subroutines become plain loads and stores.
- `lcobound`, `ucobound`, `coshape` and `image_index` follow from the
  codimension declaration.

Most of the work is in the front end:

- **Parsing:** `[...]` after a designator is an image selector, while elsewhere
  it is an array constructor. The lexer and grammar must tell these apart, and
  must also handle `codimension[*]` and `a(10)[*]`.
- **Image-control statements,** with their `STAT=` and `ERRMSG=` specifiers.
- **Constraints:** a coarray must be SAVE, allocatable or a dummy argument; a
  coindexed object cannot appear in a pure procedure; and so on.

Stage 1 works in the native compiler, the interpreter and the browser build.

- **Benefit:** compatibility only, with no speedup. Coarray code written for
  other compilers compiles and runs.
- **Testing:** gfortran `-fcoarray=single` is the reference, and the gfortran
  testsuite's `coarray/` directory has many cases.

### Stage 2: real images on one machine, with shared memory

`rfort run -n 4 prog.f90`, or an environment variable read by a built
executable, launches N copies of the program, one process per image.

**Why processes rather than threads:** with threads, every SAVE, module and
COMMON variable would have to become thread-local, which touches all of
codegen. With processes, the generated code is unchanged except for coarray
accesses.

**Symmetric shared memory:** each image maps a shared segment
(`CreateFileMapping` on Windows, `shm_open`/`mmap` on Unix).

- A static coarray has the same offset in every image's segment, so `x[j]` is
  image j's base address plus that offset. Coindexed access is then a load or
  store through a computed pointer, with no message passing.
- Allocatable coarrays come from a symmetric heap. ALLOCATE and DEALLOCATE of
  a coarray are collective and synchronize the images, so every image makes
  the same allocations in the same order and gets the same offsets.

**Synchronization:** the Fortran memory model orders coindexed accesses only at
image-control statements.

- Plain loads and stores are fine between those statements.
- Each SYNC needs fences plus a barrier or counter in shared memory.
- CRITICAL, LOCK and EVENT are atomics in the segment. A waiting image uses
  `WaitOnAddress` on Windows or a futex on Linux.

**Collectives:** `co_sum` and the others are a reduction tree, or, for small N,
a gather through shared memory.

**Termination and I/O:**

- ERROR STOP must end every image.
- A normal STOP waits for all images to finish.
- Only image 1 reads standard input.
- Output from all images needs merging that keeps each line intact.

**Deferred:** failed images, teams (FORM TEAM, CHANGE TEAM) and stopped-image
queries. At first, `failed_images()` can return an empty array.

**Interpreter and browser:**

- The interpreter could run images as threads, because each interpreter
  instance owns its own state. That is a lower priority, since the focus is the
  compiler.
- The browser build stays single-image. Browser threads need SharedArrayBuffer
  and special server headers (COOP and COEP).

### Stage 3: distributed memory (not planned)

Running across machines means targeting a communication library.

- **Route:** the current standard route is PRIF (the Parallel Runtime Interface
  for Fortran, from Berkeley Lab). LLVM Flang uses it through the Caffeine
  runtime on GASNet. rfort could lower coarray operations to PRIF calls through
  Cranelift's C ABI.
- **Cost:** this brings in an external runtime and launcher, against rfort's
  self-contained design.
- **When:** only if someone needs clusters.

## Would Stages 1 and 2 speed up programs?

Stage 1 alone gives no speedup. Stage 2 does, and Stage 3 is not needed to get
it. Ordinary machines have 8 to 24 or more cores, and rfort currently uses one.
Stage 3 adds speed only for programs too large for one machine.

**Programs that would benefit:**

- **Independent work split by image:** Monte Carlo runs, parameter sweeps and
  loops over independent cases. Each image takes `this_image()`,
  `this_image()+num_images()`, …, and a `co_sum` at the end combines the
  results. The speedup is close to linear in the number of cores.
- **Domain decomposition:** stencil, PDE and particle codes that exchange halo
  cells with neighbours each step through `x(:)[left]` and `x(:)[right]`. The
  speedup is good when each exchange comes with plenty of computation.
- **Dense linear algebra written in coarray style.** This is rare; most code
  calls a threaded BLAS instead.

**Programs that would not benefit:**

- Programs with fine-grained synchronization, such as a SYNC inside a short
  loop.
- Serial programs. Most existing Fortran uses no coarrays; a program must be
  written for them.
- Loops limited by memory bandwidth, which stop scaling after a few cores under
  any parallel model.

## Is it worth doing?

**For coarrays:**

- Coarrays are the parallel model inside the Fortran standard.
- Supporting them gives multicore speed while rfort stays self-contained: no
  OpenMP runtime, MPI or external library.
- The shared-memory design keeps coindexed access cheap, with costs only at
  SYNC points.

**Against coarrays:** little real code uses them. Parallel Fortran in practice
is mostly OpenMP, then MPI, with coarrays a distant third.

## Alternatives

- **Parallel DO CONCURRENT.**
  - **Benefit:** existing code speeds up without being rewritten, and rfort
    already analyzes these loops.
  - **Implemented:** an opt-in pool and outlined workers for a conservative
    subset including scalar and memory-budgeted fixed/runtime-sized array reductions, multiple indices, signed strides, invariant subscript offsets, multidimensional arrays, and assumed-shape procedure arguments and proved PURE/SIMPLE scalar-result calls with scalar or read-only array inputs, with serial fallback.
    Called PURE/SIMPLE functions can use private fixed-size, automatic, and allocatable numeric/LOGICAL workspaces and fixed/automatic/allocatable array results, including nested calls and disjoint section output.
  - **Next:** broader capture analysis and allocation control specifiers in called PURE functions,
    and additional thread-safe intrinsics.
    SAVE and module variables retain their Fortran sharing semantics; making
    them all thread-local would change program behavior. Unsupported captures
    should use the serial fallback with an explanatory diagnostic.
  - **Size:** still a smaller change than OpenMP.
- **OpenMP `parallel do` and reductions.**
  - **Benefit:** helps the most existing code.
  - **Cost:** OpenMP is a separate specification, and supporting it is a larger
    effort that needs the same thread-safety work.

## Recommendation

- **For speed in existing programs:** parallel DO CONCURRENT probably does the
  most good per unit of work.
- **For standard parallel Fortran in a self-contained compiler:** Stages 1 and 2
  are worthwhile, and neither needs Stage 3. Use processes and symmetric shared
  memory.
- **Stage 1 is cheap** and worth doing for compatibility either way.
- **Postpone** teams, failed images and distributed memory.

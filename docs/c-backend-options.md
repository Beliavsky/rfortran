# A C backend for rfort

Planning notes, November 2026. Nothing here is implemented. The related notes on
parallelism are in `docs/parallelism-options.md`.

The idea: translate Fortran to C and compile it with gcc or clang, as an
optional backend alongside Cranelift and the interpreter. It would not replace
either of them.

## Why it is doable

- **The IR keeps the program's structure.** It has functions, IF, DO, DO WHILE,
  SELECT CASE, BLOCK, EXIT and CYCLE (`src/ir.rs`), which map almost one to one
  onto C. A C emitter would not need a structurer (the step that rebuilds loops
  and ifs from raw jumps).
- **The runtime helpers are most of the work.** The lowered IR uses about 470
  `@...` helper calls, for formatted I/O, allocation, characters, file handling
  and so on. `src/codegen/cranelift.rs` (about 4,700 lines) expands them inline.
  - A C backend needs each one as a C function, which amounts to a runtime
    library.
  - Alternatively, the Rust runtime logic could be built as a static library
    with a C API.
  - This is likely more work than the emitter itself.
- **Precedents:** f2c translated Fortran 77 to C for decades, and LFortran has a
  C backend.

## Why it makes sense

- **Speed.** Cranelift is built to compile quickly, not to optimize hard. It
  does little loop optimization, no auto-vectorization, and less inlining and
  alias analysis than gcc or clang at `-O2`/`-O3`.
  - On numeric loops, gcc output is often noticeably faster.
  - Fortran's rule that dummy arguments don't alias can be passed to C as
    `restrict`. Much of Fortran's speed advantage comes from that rule.
- **Parallelism almost for free.** DO CONCURRENT can become
  `#pragma omp parallel for` (or `omp simd`), compiled with `-fopenmp`. That
  avoids writing a thread pool.
  - SAVE and module variables still need checking for thread safety.
  - gcc's OpenMP runtime handles the threads and scheduling.
- **Portability.** Any platform with gcc or clang works: Linux, macOS and ARM.
  The native backend currently produces Windows x86 and x64 objects with bundled
  libraries.
- **Debugging.** The generated C is readable, and sanitizers (ASan, UBSan) are
  available.

## Costs and pitfalls

- **Self-containment.** rfort would require gcc or clang, so this backend should
  be optional. Cranelift and the interpreter stay the default.
- **Compile time.** gcc is much slower than Cranelift. That suits `-O2` release
  builds, not edit-and-run.
- **Three execution paths must agree:** the interpreter, Cranelift and C.
  - The test suite already compares the interpreter, `-O0` and `-O2`, so adding
    a C column is mechanical.
  - Each new feature costs more to support.
- **Semantics to watch in C:**
  - Signed integer overflow is undefined behaviour in C, which the optimizer
    can exploit. Use `-fwrapv`.
  - Contracting `a*b+c` into a fused multiply-add changes results. Use
    `-ffp-contract=off` to match the other backends.
  - Fortran's evaluation rules and IEEE exception flags need care.
- **C language gaps:**
  - REAL(16) needs `__float128` or `long double`.
  - COMPLEX needs C99 `_Complex`, which MSVC lacks, so target gcc and clang
    only.
  - C has no nested functions, so internal procedures need an explicit static
    link. `host_storage.rs` may already lower host association to storage.

## Reusing Fortran-to-C (xf2c)

[Fortran-to-C](https://github.com/Beliavsky/Fortran-to-C) is a Python
source-to-source transpiler that emits C. This assessment uses the GitHub
version, last committed on 4 April 2026.

### The translator (`xf2c_core.py`, about 21,000 lines of Python)

rfort cannot reuse it directly. xf2c scans Fortran text and generates C from
patterns. rfort has a full parser, semantic checks and a structured IR, so its
C backend would emit C from the IR, in Rust. Two parts of xf2c are useful as
reference:

- The C idioms it uses for lower bounds, sections, complex numbers and derived
  types.
- Its practice of running gfortran and the generated C side by side and
  comparing outputs, with its corpus of example programs. rfort's test suite
  works the same way.

### The runtime (`fortran_runtime.c`, 1,323 lines)

**Parts that could be reused almost as they are:**

- `date_and_time`, `cpu_time` and `system_clock`
- command-line arguments and `getcwd`
- `scan`, `verify` and `index`
- the MT19937-64 generator. It is usable only if every rfort backend switches
  to it, since `random_number` must give the same sequence in all three
  execution paths.

**Parts that do not fit rfort's semantics:**

- **Strings are NUL-terminated `char *`.** Fortran strings carry an explicit
  length and may contain `char(0)`. rfort's backends use pointer-and-length
  strings.
- **String temporaries come from a ring of 8 shared buffers.**
  - An expression with a ninth live temporary silently overwrites the first.
  - The ring is not thread-safe, which rules it out for OpenMP.
- **There is no general formatted I/O engine.**
  - Formats become `printf` calls, plus narrow pattern helpers such as
    `write_i0_then_words` and `read_int_float_record`.
  - `printf` differs from Fortran edit descriptors (G, ES, list-directed
    spacing and so on).
  - rfort's C output must match its interpreter and Cranelift output byte for
    byte. rfort already has that engine in Rust (`format_spec.rs`, `input.rs`).
- **Units are a simple table of `FILE *` handles.** rfort's compiled code already
  supports IOSTAT, ACTION checks, NEWUNIT, non-advancing I/O, direct access,
  stream POS and INQUIRE.
- **The array helpers handle only 1-D contiguous arrays.** A C backend should
  emit the loops directly, so gcc can inline and vectorize them.

### What rfort's C runtime should be

It must reproduce exactly what the roughly 470 `@` helpers do in the Cranelift
backend.

- **Share the runtime between the native backends.** Move that logic into one
  runtime used by both, written in C or as Rust built as a static library with
  a C interface, so the two cannot drift apart.
- **Take from xf2c** the clock, argument, date and RNG functions.
- **Leave out** its string, I/O and unit handling.

### Conclusion on xf2c

The transpiler shows the approach works and has a corpus of programs worth
reusing. It does little to shorten the main work: emitting C from the IR and
building a runtime with full semantics.

## Plan

1. **Measure first.** Run a few numeric benchmarks with rfort `-O2` against
   gfortran `-O2`.
   - If rfort is within about 20–30%, the speed case is weak.
   - If it is 2–3 times slower, a C backend is the cheapest way to close the
     gap. An LLVM backend would be a much heavier dependency.
2. **Prototype.** Add a `--backend c` that handles scalars, arrays, loops, PRINT
   and a handful of helpers. Measure its benchmark speed, and compare its
   output with the other execution paths.
3. **If it pays off,** fill in the helpers. Preferably share one runtime library
   with Cranelift.

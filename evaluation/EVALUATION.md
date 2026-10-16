# rfortran evaluation — 2026-10-16

Evaluation of the current state of rfortran (`rfort`) at commit `20d9a6c`, on
Windows 11 x64, compared against gfortran. No compiler source was modified.
Test programs are in [`tests/`](tests/).

## Summary

Further work on rfortran could benefit the Fortran community, though probably
not by competing head-on with gfortran or ifx for years. The case is strongest
if it builds on what it already does better than other compilers — catching
bugs such as reads of unset variables — and on its value as a demonstration
of LLM-built compilers.

## What was measured

### Build and unit tests

- `cargo build --release --bin rfort` succeeded (about 5½ minutes).
- `cargo test --release` exited cleanly.

### ofort compatibility suite

Command:

```
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --discover --jobs 6 --report ofort-full.json
```

Result: **132 of 440 cases pass** at both `-O0` and `-O2`, up from 62 recorded
in the README on 2026-10-11. (31 cases failed to compile with gfortran and 3
failed or timed out when run; those are excluded from the 440.)

Of the 308 failures:

| Failure group | Count |
|---|---|
| Parse error | 167 |
| Other compile error | 105 |
| Name resolution error | 16 |
| Unsupported feature | 16 |
| Wrong output | 3 |
| Runtime error | 1 |

Only 4 failures produced wrong results or crashed at run time. When rfort
compiles a program, it almost always gets it right.

Pass rate by feature area:

| Area | Passing |
|---|---|
| Array operations | 34 / 48 |
| Character | 9 / 12 |
| Optional arguments | 3 / 4 |
| Procedures and modules | 18 / 43 |
| Language basics | 25 / 60 |
| I/O and formats | 10 / 26 |
| Numeric and intrinsic | 12 / 42 |
| Allocation | 11 / 92 |
| Pointers | 9 / 64 |
| Derived types | 1 / 17 |
| Procedure pointers | 0 / 32 |

### Typical modern Fortran programs (`tests/p*.f90`, `tests/p9_legacy.f`)

Ten small programs representing everyday code were compiled with both
compilers using `tests/compare_with_gfortran.sh`. **Two compiled**; both
matched gfortran's output values. List-directed formatting differs (for example
`55.0` versus `55.000000000000000`), which the standard allows.

| Program | Feature tested | Result |
|---|---|---|
| `p10_generic.f90` | Elemental generic `swap` | ✅ |
| `p1b.f90` | Allocatable arrays, reallocation (variant of p1 without `spread`) | ✅ |
| `p1_runtime_alloc.f90` | `spread` of an allocatable | ❌ "runtime PACK results are not yet supported as arguments to `spread`" (misleading message) |
| `p2_dtype_alloc.f90` | Allocatable components, `character(len=:)` component | ❌ parse error at `contains` |
| `p3_assumed_shape.f90` | Named `block` | ❌ parse error |
| `p3b.f90` | `reshape([(real(i,8), i=1,n*n)], [n,n])` with variable `n` | ❌ "`n` is not a previously defined integer PARAMETER" |
| `p4_oop.f90` | `type, abstract`, `class(shape)`, deferred bindings | ❌ parse error |
| `p5_internal_io.f90` | `write(10,*)` to an external file | ❌ "external file WRITE currently requires an explicit format" |
| `p6_recursion.f90` | Reallocation on assignment, recursive quicksort | ❌ "array `a` is used before ALLOCATE" |
| `p7_strings.f90` | `character(len=:)` passed to a function | ❌ "CHARACTER actual argument requires a fixed length" |
| `p8_ode.f90` | `abstract interface`, procedure dummy (RK4 solver) | ❌ parse error |
| `p9_legacy.f` | Fixed-form FORTRAN 77 | ❌ input type rejected |

`tests/q1.f90` confirms that allocatable arrays sized at run time (from a
command-line argument, up to 10,000,000 elements) work, including passing them
to an assumed-shape dummy.

### Uninitialized-variable detection (`tests/u*.f90`)

| Program | Situation | gfortran `-Wall -Wextra -O2` | rfort |
|---|---|---|---|
| `u1.f90` | Scalar conditionally set in a loop | Warning | Runtime error with line number |
| `u2.f90` | `intent(out)` argument unset on one branch | **No warning** | Runtime error: ``read of uninitialized variable `z` at u2.f90:13`` |
| `u3.f90` | Array with one element unset | — | Runtime error: ``element 5 at u3.f90:6`` |
| `u4.f90` | Definite read before assignment | — | **Compile-time error** with source span |

This is a genuine strength. The `-Woptional-argument` check (OPTIONAL
arguments used without a proven `present` guard) is also not offered by
mainstream compilers.

### Performance (`tests/b*.f90`)

Both compilers at `-O2`:

| Benchmark | gfortran | rfort | Ratio |
|---|---|---|---|
| `b2.f90`: scalar loop, 200M `sqrt` calls | 1.2 s | 6.2 s | ~5× slower |
| `b3.f90`: 400×400 triple-loop matrix multiply | 0.76 s | 12.0 s | ~16× slower |

There is no option to disable the runtime uninitialized-read checks, and
Cranelift does much less loop optimization than GCC. It is not yet usable for
production numerical work.

## Assessment

### Evidence for the LLM-in-Rust hypothesis

About 44,000 of the roughly 54,000 lines of Rust were added between
2026-10-09 and 2026-10-16, across 167 commits. The ofort pass count doubled
in five days. The near-zero wrong-output rate suggests the workflow (gfortran
as the reference, extensive regression tests) produces correct code, not just
code that compiles.

### Main risk: architecture

The README states that procedure calls use specialization rather than a
general descriptor ABI and that many array shapes must be known at compile
time. The source contains over 200 "not yet supported" / "currently requires"
diagnostics, for example:

- "sections of bounded allocatable arrays are not yet supported"
- "ALLOCATE currently requires a shape known at the call site"
- "subroutine array actual arguments are not yet supported; pass scalar arguments"
- "recursive specialized subroutine calls are not yet supported"

Source files named after individual features (`empty_constructors.rs`,
`character_elementals.rs`, `recursive_arrays.rs`) point the same way. This
looks like growth by special case rather than through general mechanisms.
Without those, each new feature multiplies the special cases, and gaps such as
polymorphism, procedure pointers and derived types with allocatable components
will get harder to close rather than easier.

## Recommendations

1. **Build the general foundations soon:** a real array descriptor ABI
   (assumed-shape, allocatable, pointer, sections), deferred-length CHARACTER,
   and runtime-shaped array constructors. Specialization should be an
   optimization layered on top, not the only path.
2. **Pick a niche where it can beat other compilers:** a strict checking
   compiler (or `rfort check` mode) for development and teaching that finds
   uninitialized reads, unguarded OPTIONAL use and unused entities, with
   production builds made by gfortran or ifx. That makes the slowdown
   irrelevant and puts the current strength to work. For this, broad parsing
   matters more than code generation, and parse errors are the largest
   failure category.
3. **Add an option to disable runtime checks** and measure how much of the
   slowdown is checks versus Cranelift code quality.
4. **Support fixed-form source**, since much existing Fortran is FORTRAN 77
   style.

LFortran and LLVM Flang already exist as open-source alternatives, so another
full compiler is not a gap by itself. What is new is the experiment: whether
LLM-written Rust can keep up the current pace. Tracking the ofort pass rate
and the small typical-program set above weekly would answer that.

## Reproducing

From `evaluation/tests/` in Git Bash, with `target/release/rfort.exe` built
and gfortran on PATH:

```sh
sh compare_with_gfortran.sh p*.f90 p9_legacy.f
```

Each program is compiled and run with both compilers, and their outputs are
compared ignoring whitespace. The `u*.f90` and `b*.f90` programs were run
directly with `rfort -o x.exe file.f90` and `gfortran -O2`.

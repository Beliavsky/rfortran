# Why Cranelift?

Cranelift is a good fit for rfortran because it makes building and evolving a
native compiler relatively manageable. Its main advantages concern compilation
speed and compiler development; LLVM and GCC offer stronger infrastructure for
maximizing numerical execution speed.

## Advantages for rfortran

- **Fast code generation.** Cranelift explicitly prioritizes compilation speed,
  which suits experimentation, small programs, and frequent edit–compile–run
  cycles.
- **Convenient Rust integration.** It is written in Rust and designed as an
  embeddable library, so it fits naturally into rfortran's implementation.
- **Manageable backend complexity.** Its comparatively simple optimization
  model makes understanding generated code and investigating problems more
  approachable.
- **AOT and JIT capability.** rfortran uses Cranelift to produce native object
  files. JIT compilation could support future interactive features. The current
  interpreter, REPL, and browser playground use a separate execution path.

These are general backend engineering benefits rather than built-in support
for Fortran. See [Cranelift's design and goals](https://cranelift.dev/).
Producing executables still requires a linker and the appropriate platform
libraries. Procedure locals live on the stack at 8 bytes per numeric element,
so Windows executables reserve a 256 MiB stack (address space only, committed
as it is used) instead of the default 1 MiB; worker threads of parallel DO
CONCURRENT get the same reserve. On Linux and macOS the stack limit comes from
the environment, for example `ulimit -s`.

## Comparison with other backends

| Backend | Main attraction for a Fortran compiler | Main tradeoff |
|---|---|---|
| Cranelift | Fast code generation and straightforward Rust integration | Less aggressive optimization |
| LLVM | Extensive optimization, vectorization, tooling, and target support | More infrastructure and integration complexity |
| GCC | Mature numerical optimization infrastructure already used by gfortran | A less straightforward fit for an independently developed Rust frontend |
| Generate C | Access to GCC or Clang optimization through a portable intermediate language | Additional translation and toolchain steps; careful handling of Fortran semantics |

LLVM provides both loop and straight-line vectorization, while GCC provides
extensive vectorization and loop transformations. These capabilities matter
substantially for numerical Fortran programs. See the
[LLVM vectorizer documentation](https://llvm.org/docs/Vectorizers.html) and
[GCC optimization options](https://gcc.gnu.org/onlinedocs/gcc/Optimize-Options.html).
GNU Fortran uses the GCC backend, as described in
[About GNU Fortran](https://gcc.gnu.org/onlinedocs/gfortran/About-GNU-Fortran.html).
The integration tradeoffs in the table are engineering judgments for rfortran.
LLVM and generated C are possible future alternatives, not additional native
backends currently available in rfortran.

## What the backend does not decide

Fortran performance depends heavily on work above the machine-code backend:
avoiding array temporaries, combining array operations, choosing efficient
runtime algorithms, and preserving information about shapes and aliasing.
Flang's higher-level Fortran representation exists partly to enable these
transformations before lowering to LLVM. See
[Flang's HLFIR design](https://flang.llvm.org/docs/HighLevelFIR.html).

Cranelift cannot compensate for inefficient lowering in rfortran's frontend.
It also does not implement Fortran array descriptors, CHARACTER semantics,
I/O, or parallel loop scheduling for us. A generated-C backend would similarly
need to preserve Fortran's numerical, aliasing, and storage semantics; emitting
C would not automatically inherit all of gfortran's Fortran-specific
optimizations.

Consequently, rfortran's benchmark differences from gfortran cannot all be
attributed to Cranelift. Fast backend code generation also does not guarantee
fast overall compilation when frontend processing dominates. Building rfort
itself with the usual Rust toolchain is a separate process from generating
machine code for user Fortran programs.

## Assessment

For an alpha compiler intended to make Fortran features easy to experiment
with, Cranelift is a useful choice. For peak HPC performance, LLVM or GCC has
the stronger starting position. An additional optimizing backend could
eventually complement Cranelift if benchmarks justify the development effort.

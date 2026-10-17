# Has Rust benefitted rfortran?

Assessment written 2026-10-16, based on building, testing and extending the parser
during the evaluation in [EVALUATION.md](EVALUATION.md).

Other Fortran compilers (gfortran, flang, LFortran) are far more mature, and their
developers also have access to LLMs. So any relative advantage for rfortran has to come
largely from being written in Rust instead of C or C++.

**Short answer:** Yes, Rust has helped, but modestly. It is not what will decide whether
rfortran succeeds.

## Where Rust clearly helped

- **Exhaustive matching made the grammar work safer.** When `TokenKind::AttrStatement`
  was added, the compiler would not build until it was handled in `human_token`. In a
  large C or C++ compiler, a new node or token kind can be missed silently in some switch
  statement. With Rust, extending the AST is checked mechanically. This matters most when
  an LLM makes many small edits across files it hasn't fully read.
- **Compile errors give a fast feedback loop.** Mistakes in Rust code (for example, a
  slice pattern on a `Vec` that caused E0529) were caught when compiling, not as wrong
  output 60 minutes into the regression suite. Of the arguments for Rust, this one is
  strongest for LLM-driven development: the type checker acts as a free reviewer.
- **The ecosystem supplied a working toolchain quickly.** logos (lexer), LALRPOP (parser
  generator) and especially Cranelift (code generator) let one project reach native code
  without depending on LLVM or GCC. Cranelift is small, embeddable and written in Rust;
  C and C++ have no comparable option. Cargo and in-crate unit tests also kept build and
  test setup simple.
- **No signs of memory corruption.** The failures seen in the compiler were clean
  diagnostics, not segfaults. One early run hung, but the cause was never found.

## Where Rust didn't help, or cost something

- **Build times are slow.** A release build took 5–6 minutes, so iteration used a
  separate debug build. Part of this is the configuration (LTO, `opt-level = "z"`, one
  codegen unit), but slow compiles are a real cost of Rust.
- **LALRPOP struggles with Fortran syntax.** It wants a context-free grammar, but Fortran
  has no reserved words and statements end at line breaks. Much of the parsing work went
  into lexer workarounds (`statement_head`, `KwType`, `BareEnd`, `EndUnitName`) that
  relabel tokens before the grammar sees them. gfortran, flang and LFortran all use
  hand-written or more flexible parsers. This came from the choice of library, not from
  Rust itself, but the Rust ecosystem encouraged it.

## Things that are not due to Rust

- **Detection of used-but-unset variables.** This feature comes from rfortran's design:
  it specializes code at compile time and analyzes the whole program. A C++ compiler
  could do the same; it just hasn't been a priority for the others.
- **The main gaps.** These are architecture and maturity, not language:
  - derived types limited to scalar components
  - no procedure arguments
  - no GO TO
  - no general array-descriptor ABI

  Several of these come from design shortcuts, and they will limit rfortran more than
  language choice helps it.

## Comparison with the other compilers

- **LLMs work on a level field.** Developers of gfortran, flang and LFortran also have
  LLMs. What actually catches mistakes in LLM-written compiler code is the test setup,
  not the language. Here that means the 1,123-test regression suite and the comparison
  runs against gfortran.
- **Rust narrows the gap but doesn't close it.** It makes each LLM edit somewhat safer
  and cheaper to verify. That is a real but incremental advantage, not one that by itself
  lets a young compiler overtake mature ones.

## Overall

Rust has been a net positive. The main benefits are Cranelift, exhaustiveness checking
and fast feedback from compile errors. The main costs are build times and an awkward
parser generator. If rfortran gains an edge, it is more likely to come from Rust combined
with diagnostics the others don't offer, such as stronger checks for undefined
variables, than from Rust alone.

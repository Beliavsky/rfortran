# Extracting a linter from rfort

Discussed: 2026-11-10. Status: deferred; no code has been changed for this.

The question was whether to split rfort's warnings into a separate linter,
in the way that [Fortitude](https://github.com/PlasmaFAIR/fortitude) is a
standalone Fortran linter in Rust. The requirements were:

- users could download only the linter;
- the checks could be developed without rebuilding the compiler;
- rfort keeps all its current `-W` options;
- rfort and the linter share one code base, with no copied checks.

## Recommendation

Use a Cargo workspace in which the compiler and a linter binary share library
crates. Both call the same check code, so the options stay identical.

```
rfortran/                 (workspace)
  crates/frontend/   lib: lexer, parser, AST, fixed-form conversion, INCLUDE
                          and preprocessing, source origins, diagnostics, and
                          module (.rmod) reading
  crates/lint/       lib: every *_warnings.rs, unused.rs, source_advice.rs,
                          reachability.rs, the option registry (CATEGORIES,
                          EXTRA, STYLE, validate_options, selected), and one
                          entry point, lint::check(program, source, options)
  rfort              bin: the compiler and interpreter, using frontend and
                          lint; calls lint::check where
                          frontend::prepare_program calls the checks now
  rflint             bin: a small command-line program using frontend and lint
```

## Benefits

- **A small download.** `rflint` would contain no code generation, runtime or
  interpreter.
- **Faster work on checks.** Release builds take 7 to 10 minutes because of
  LTO over the whole crate. `cargo test -p lint` and `cargo build -p rflint`
  would compile only the frontend and the checks. rfort would still need
  rebuilding to pick up a change, but not during development of a check.
- **Clearer boundaries.** The checks would have to state which compiler
  internals they use.

## Risks and costs

- **How entangled the checks are.** Some checks run after
  `module_files::prepare`, which resolves modules. `source_advice.rs` and
  `value_warnings.rs` call compiler modules such as `procedure_arguments`,
  `parameters::logical_constant` and `reductions::array_result`. If these
  pull in lowering passes, the frontend crate grows, or the helpers must move
  down into it. Measure the transitive module dependencies of the warning
  modules first. That decides whether the frontend is about 20 files or 80.
- **A large mechanical diff.** The crate uses `include!("frontend_modules.rs")`
  and `crate::` paths throughout. Do the move when no other work has
  uncommitted changes, to avoid conflicts.

## What a linter needs beyond the compiler's warnings

1. **Graceful degradation.** A linter is expected to report on code the
   compiler rejects or does not support yet. The token-level checks need no
   AST: -Windentation, -Wearly-return, -Wliteral-kind, -Wdefault-real,
   -Wdimension-override and the -Wstyle checks in `style_warnings.rs`. These
   could still run when parsing fails, and the AST checks would be skipped
   with a note. This matters because rfort does not parse all of Fortran.
2. **Configuration**, in a `rflint.toml` or a section of `fpm.toml`, listing
   the checks to enable and disable. The existing `!#allow(...)` comments
   would still work.
3. **Output formats**: the current human-readable text, plus JSON and GitHub
   annotations for CI.
4. **Stable rule codes**, like Fortitude's, alongside the names, for
   configuration files and documentation. This is optional.
5. **Automatic fixes.** Many checks already produce exact suggestions, such as
   adding `::`, replacing `.lt.` with `<`, ONLY lists, `end subroutine s` and
   `real(dp)`, so `rflint --fix` could apply them.

## Compared with Fortitude

Fortitude uses tree-sitter, so it parses almost any Fortran without errors,
and it has a Ruff-like interface with rule codes, configuration and fixes.
rfort's checks use real semantic analysis: types, constant folding, module
contents and control flow. That makes checks such as REAL-context integer
division, -Wcould-be-parameter, -Wcompare-reals, ONLY-list suggestions and
-Winfinite-loop accurate. The two are complementary. As
`FORTITUDE_REUSE.md` notes, some of Fortitude's checks and test cases are
worth adapting, provided its MIT notices are kept.

## Plan

Each step leaves rfort passing the full test suite.

1. Map the modules the warning code depends on, and report the result. This
   step only reads code.
2. Convert the repository to a workspace with the frontend crate. rfort's
   behavior does not change.
3. Move the checks and the option registry into the lint crate, and have
   rfort call it.
4. Add the `rflint` binary, starting with the same options and output as
   `rfort check`, and running the token-level checks when parsing fails.
5. Add configuration, JSON output, rule codes and `--fix`.

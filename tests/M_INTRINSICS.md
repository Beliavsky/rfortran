# Testing rfort with M_intrinsics examples

`run_m_intrinsics.py` uses the standalone demos in an external
[M_intrinsics](https://github.com/urbanjost/M_intrinsics) checkout. The project
extracts these examples from its intrinsic documentation; they are examples,
not a comprehensive conformance suite or a collection of assertion-based tests.

The runner does not edit upstream examples, rebuild rfort, invoke the
interpreter, or change compiler/transpiler sources. It copies source bytes into
temporary directories and snapshots the selected rfort executable so another
build cannot change the compiler halfway through a report. Only an existing
native compiler's command-line interface is used; this checkout has no separate
transpile command.

## Compilation inventory

From the rfortran repository, in Windows CMD:

```cmd
python tests\run_m_intrinsics.py --intrinsics-root C:\fortran\public_domain\github\M_intrinsics --opt-level 0 --jobs 2 --report target\m-intrinsics-compile.json
```

This compiles all discovered demos with rfort and gfortran, without running the
generated programs. Without `--opt-level`, rfort is tested at both O0 and O2.
Use `--compiler path\to\rfort.exe` to select a particular existing build; the
default is `target\release\rfort.exe`. Use `--no-reference` for a compilation
inventory without gfortran, or `--reference` to select another gfortran binary.

## Execution comparisons

```cmd
python tests\run_m_intrinsics.py --intrinsics-root C:\fortran\public_domain\github\M_intrinsics --run --filter demo_abs.f90 --filter demo_fraction.f90 --filter demo_exponent.f90 --filter demo_spacing.f90 --report target\m-intrinsics-run.json
```

`--filter` and `--exclude` are repeatable filename globs. `--limit` can select
the first N matching examples. `--reference-arg=-fcoarray=single`, for example,
adds an explicit reference compiler option where needed; it is not enabled by
default. `--timeout` limits each compilation or execution stage.

With `--run`, eligible examples are executed with gfortran twice in fresh working
directories. Stable reference output is compared with each native rfort build.
The comparison reuses the existing compatibility runner's value comparator:
whitespace/layout differences and modest real roundoff are tolerated, while
integer values are compared exactly. This is not a formatting-conformance test.
Matching output is evidence for the particular example, not proof of complete
intrinsic support. Two matching reference executions cannot establish that an
example is deterministic under every condition.
Conversely, failure to compile a demo does not necessarily mean its named
intrinsic is missing: the same program may require an unrelated language feature
such as DO CONCURRENT, a particular declaration spelling, or an unsupported kind.

Programs involving randomness, clocks, external commands, environment queries,
READ/file fixtures, interop, or other identified dependencies are still compiled
but conservatively excluded from automatic execution. These examples need
tailored inputs or property-based expectations. The scan is conservative triage,
not an execution sandbox. Review source before executing unfamiliar examples.
The upstream example set includes a random-number demonstration with a
100-million-iteration loop, illustrating why indiscriminate execution is not a
useful default.

The library/application tests under upstream `test/` and the documentation CLI
under `app/` are not selected: they exercise the documentation package and its
external module dependencies rather than isolated intrinsic examples.

## Reports

Each run writes a detailed JSON report and a companion Markdown summary. JSON
is checkpointed as cases finish. Reports record the upstream commit, source
hashes, compiler executable hashes and versions, commands, timings, exit codes,
diagnostics, reference outcomes, runtime exclusions, and per-optimization status.

- `compiled`: compilation succeeded; execution was not requested.
- `pass`: execution completed and output matched the reference.
- `compiled_run_skipped`: a demo compiled but needs a tailored runtime test.
- `compiled_no_reference_oracle`: compilation succeeded but reference compilation
  or execution failed, or repeat reference output differed.
- `run_ok_unchecked`: execution succeeded without a reference comparison.
- `wrong_output`: native execution disagreed with the reference.
- Compile/run errors, crashes, launch failures, and timeouts are distinguished.
- `mixed`: optimization levels had different statuses; inspect `results`.

A gfortran rejection does not prove an example is invalid: its language version,
supported kinds, optional facilities, or external dependencies may differ.
Reference failures never suppress rfort compilation, and are not silently
counted as rfort execution passes. Warnings are retained in the report.

`--strict` returns nonzero on rfort failures. In execution mode it requires every
selected optimization result to be `pass`, so skipped or unchecked runs also
produce a nonzero strict exit status. Without `--strict`, ordinary compatibility
failures are recorded without stopping the inventory.

Run the runner's own tests without building the compiler:

```cmd
python -m unittest discover -s tests -p test_m_intrinsics_runner.py
```

## Initial baseline: 2026-10-17

This baseline used the existing release executable, not the concurrently edited
working sources. No compiler build or source change was made for these runs.

- Upstream HEAD: `3e9cade230bf1197f8dc41782df95e772909e28c`.
- Compiler: rfort 0.1.0, `x86_64-pc-windows-msvc`.
- Compiler SHA-256:
  `61bf14049e9318d2cd08c5fe0deb9cbffcf0526549c8bcdeed68a2a530cb314d`.
- Reference: GNU Fortran 17.0.0 20260510 (experimental).
- Runner unit tests: 11 passed, including process timeout cleanup.

The full inventory covered all 249 upstream demos at rfort O0:

| Outcome | Count |
|---|---:|
| rfort compiled | 25 |
| rfort compilation error | 224 |
| gfortran compiled | 204 |
| gfortran compilation error | 45 |

These counts describe complete demo programs, not the number of implemented
intrinsics. For example, `demo_abs.f90` hits the `doubleprecision` declaration
spelling, `demo_achar.f90` hits DO CONCURRENT, and `demo_acos.f90` also exercises
COMPLEX arguments. `demo_adjustr.f90` is blocked by output-format handling.
These require reduced repros before attributing failures to intrinsic support.

All 25 demos that compiled in the inventory were then checked at both O0 and O2:

| Execution outcome | O0 | O2 |
|---|---:|---:|
| Output matched gfortran | 22 | 22 |
| Runtime error | 1 | 1 |
| Runtime deliberately skipped | 2 | 2 |

The runtime error is in `demo_asin.f90`, with the diagnostic
`invalid output format: descriptor does not match output item type`. The example
uses a format containing G0 descriptors. This is a formatting failure, not
evidence that the ASIN calculation is wrong. No implementation fix was attempted.
The skipped programs are `demo_cpu_time.f90` and `demo_system_clock.f90`, whose
outputs require timing-specific expectations.

Detailed local reports (generated under the ignored `target/` directory):

- `target/m-intrinsics-compile.json` and `.md`: full compilation inventory.
- `target/m-intrinsics-compiled-run.json` and `.md`: execution checks for all 25
  compilable demos.
- `target/m-intrinsics-run.json` and `.md`: an earlier selection of 30 demos,
  with six execution passes and 24 compilation errors at each optimization level.

Re-run with the desired executable after the independent compiler/transpiler
work is built; the executable hash distinguishes that later result from this
baseline.

## After the keyword-spelling and literal fixes: 2026-10-17

The baseline executable predates two commits from the same day (`c96b384`,
automatic arrays and intrinsic rewrites; `1cebd4f`, common statement forms).
The inventory was re-run with a release build of `1cebd4f`
(SHA-256 `a5e0c83be896191d02c8cb44952692a34f2e7eef5a1eb77aceb55c530c8468b6`),
then with a debug build of the working tree that adds:

- one-word keywords: `ENDIF`, `ELSEIF`, `SELECTCASE`, `ENDSELECT`, `ENDBLOCK`,
  `ENDFUNCTION`, `ENDSUBROUTINE`, `ENDPROGRAM`, `ENDMODULE`, `ENDTYPE`,
  `ENDINTERFACE`, `ENDASSOCIATE`, `ENDFORALL` and `DOUBLEPRECISION`;
- BOZ literal constants (`b'0101'`, `o'17'`, `z'ff'`), with `INT(boz, KIND)`
  for constant arguments;
- operators directly after an integer literal (`i>=97.and.i<=122`);
- `BGE`, `BGT`, `BLE` and `BLT`.

Writing `endif` as one word previously failed to parse. The diagnostic pointed
at the following line, which hid the cause in about 19 demos.

Upstream HEAD is unchanged (`3e9cade`), and gfortran still compiles 204 of the
249 demos. A "parse error" below is a first diagnostic of the form
`unexpected ...` or `unrecognized token`. A "later error" is any other
compilation error, such as an unsupported intrinsic, descriptor or kind.

| rfort O0 outcome (all 249 demos) | Baseline | `1cebd4f` | Working tree |
|---|---:|---:|---:|
| Compiled | 25 | 46 | 48 |
| Parse error | 137 | 114 | 67 |
| Later error | 87 | 89 | 134 |

| Same, for the 204 demos gfortran compiles | Baseline | `1cebd4f` | Working tree |
|---|---:|---:|---:|
| Compiled | 25 | 33 | 35 |
| Parse error | 96 | 88 | 47 |
| Later error | 83 | 83 | 122 |

Most demos have more than one obstacle, so fixing the parse error usually
exposes a later one rather than completing the build. Thirteen of the demos
that compile are empty upstream files. rfort treats a file with no main
program as a compilation-only unit and writes an object file, while gfortran
fails to link it.

Execution checks of the 48 demos that compile, at both O0 and O2:

| Execution outcome | O0 | O2 |
|---|---:|---:|
| Output matched gfortran | 25 | 25 |
| Runtime error | 5 | 5 |
| Runtime deliberately skipped | 5 | 5 |
| No reference (empty upstream files) | 13 | 13 |

No demo produced output that disagreed with gfortran. The runtime errors are
all output formatting: `demo_asin.f90` uses G0 with a non-REAL item, and
`demo_lge.f90`, `demo_lgt.f90`, `demo_lle.f90` and `demo_llt.f90` need format
reversion.

The most common remaining first diagnostics, among demos gfortran compiles:

| Cause | Demos |
|---|---:|
| Missing elemental intrinsics (`ACOSD`, `SIND`, `ASINPI`, `ASINH`, `ACOSH`, `AINT`, ...) | 22 |
| Output descriptors `B`, `O`, `Z` and formats held in CHARACTER variables | 17 |
| Named BLOCK constructs (`ARRAYS: block`) | 12 |
| `ISO_FORTRAN_ENV` entities not provided (`integer_kinds`, `compiler_version`, `output_unit`, `error_unit`, `iostat_eor`) | 11 |
| `allocate(character(len=n) :: s)` | 6 |
| Array initializers using `RESHAPE` or implied-DO | 6 |
| Keyword arguments to intrinsics (`MOVE_ALLOC`, `NORM2`, `PARITY`, ...) | 6 |
| `KIND(0.0)` or a named kind in a type parameter; `DO CONCURRENT` | 5 |
| Assumed-rank dummies (`a(..)`) | 5 |

Reports for these runs are `target/m-intrinsics-compile-head.json`,
`target/m-intrinsics-compile-step1.json` and
`target/m-intrinsics-compiled-run-step1.json`, each with a companion `.md`.

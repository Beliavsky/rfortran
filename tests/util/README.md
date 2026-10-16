# util.f90 procedure coverage

These small drivers exercise the external `util_mod` used by `xutil.f90`.
The initial suite contains 30 drivers targeting 116 specific procedures, starting
with the families used by that program. The inventory covers all 551 procedures
in the current source, including two internal procedures. The remaining 435
procedures are explicitly untested. These are procedure counts, not estimates of
compiler completion or branch coverage.

## Current baseline

On Windows, using the release compiler build, the 30 drivers have the same
outcomes at both O0 and O2:

| Driver outcome | Count |
| --- | ---: |
| Pass against gfortran | 23 |
| Compilation failure | 7 |
| Incorrect output | 0 |

All gfortran reference drivers compile and run successfully, including their
repeat executions. The passing drivers target 83 procedures; 33 other targeted
procedures are in blocked groups. `string_fields` and `partitions_and_runs` now
pass following fixes to early RETURN in explicit-step loops and NINT rounding.
`matrix_shapes` and `cumulative_sums` now pass after routing runtime selectors
and triplets on fixed storage through the runtime section path. `diagonals`
now passes with runtime FORALL bounds and strides. `unique_characters` now passes
after preserving active CHARACTER array extents and specializing nested search
functions. Its array-valued search also uses descriptor-backed FORALL snapshots.
`formatted_output` now passes after fixes to runtime standard-output units,
runtime-format array items, ADVANCE expressions and empty CHARACTER lists. Its
driver also exercises headers, trailers and embedded quotes.
`numeric_vectors` now passes after adding numeric MIN/MAX to array-expression
lowering, including scalar reduction results used as array operands. The driver
covers zero normalization, an empty logical fraction, and empty/negative sequences.
`sorting` now passes after implementing scalar and elemental GNU ISNAN. Its
driver covers both sorting paths, duplicates, ascending/descending/original
ordering, empty inputs, singletons and the NaN early-return path.
See [baseline.json](baseline.json)
for exact input-source and compiler hashes and every procedure's inventory entry.

The `string_matching` driver passes in isolation, while full-module compilation
currently stops at `INQUIRE(...,OPENED=...)` in `util.f90:4428`. This demonstrates
why isolated successes and full-module integration are recorded separately.

## Running

The original `kind.f90` and `util.f90` are external inputs; they are not copied
into this test directory. By default the runner reads them from the repository
root. Use `--kind-source` and `--util-source` for other locations. It does not
modify either file. Python 3.10+, gfortran, and a built rfort are required.

From the repository root in CMD:

```cmd
cargo build --release --bin rfort
python tests/run_util_coverage.py
```

The default run compares native rfort output at `-O0` and `-O2` with gfortran.
gfortran uses its default dialect, allowing the original module's GNU intrinsics
such as `getcwd`, and `-fcheck=all`. Each compiler runs in its own temporary
directory. Reference execution is repeated to detect unstable output. Numeric
values and logical values are compared while allowing formatting differences
and real roundoff. Character-array drivers use explicit separators where needed
to avoid treating list-directed spacing differences as incorrect values.

Random and clock tests print invariant checks, not random sequences, timestamps,
executable paths, or elapsed durations. File tests receive independent fixtures.
The suite includes empty arrays, absent optional arguments, keyword arguments,
repeated allocations, and several ranks and kinds. It does not exhaust every
procedure's inputs or branches.

## Isolation and integration

In the default `isolated` mode, the runner extracts the selected procedure bodies
**unchanged** from the current utility source. It conservatively adds dependencies,
including every specific of a referenced generic, and retains module declarations
and constants. It filters generic interfaces and removes module access restrictions
so private specifics can also be tested. It preserves nested procedures inside their
host. This separates local functionality from blockers in unrelated procedures.

The extractor is a scanner for this free-form source, not a Fortran parser or
type-aware dependency resolver. It recognizes calls, function-like references and
defined operators; additional procedure-valued dependencies can be listed in a
case's `dependencies` field. Unused generic specifics can still cause a group's
compilation to fail. A successful gfortran build checks the generated source, but
does not establish that an extracted group is equivalent to full-module compilation.

Use the original module, with its original privacy and all procedures, for an
integration check:

```cmd
python tests/run_util_coverage.py --case string_matching --mode both
```

`--mode module` runs only the full-module version. Full-module cases can encounter
the same unrelated blocker; this is recorded separately from isolated results.
The existing `rfort kind.f90 util.f90 xutil.f90 --run` remains the end-to-end goal.

## Reports and reproducers

```cmd
python tests/run_util_coverage.py --inventory-only --report target/util-inventory.json
python tests/run_util_coverage.py --case string_matching --case same_shapes --strict
python tests/run_util_coverage.py --case "*char*" --compiler target/debug/rfort.exe
python tests/run_util_coverage.py --snapshot tests/util/baseline.json
python tests/test_util_coverage_runner.py
```

The complete report is `target/util-coverage.json`. It records compiler and source
hashes, reference compiler identity, included dependencies, compiler diagnostics,
runtime output, and per-mode results. Failures distinguish compilation, runtime,
crashes, timeouts, reference problems, and output mismatches. `--strict` returns a
nonzero exit status for any selected failure; the default permits collecting a
baseline containing known failures. `baseline.json` is a compact snapshot for the
specific source hashes recorded in it, not a universal expected-results file.

Every run retains standalone source groups under
`target/util-repros/<case>/<mode>/`. For example, with rfort on PATH:

```cmd
pushd target\util-repros\string_fields\isolated
rfort kind.f90 util.f90 driver.f90 --run
popd
```

Procedure inventory status means:

| Status | Meaning |
| --- | --- |
| `pass` | All selected driver/optimization/mode results targeting it passed. |
| `partial` | At least one passed and another failed. |
| `blocked` | Associated drivers failed; the cause can be this procedure, a dependency, or a compiler feature. |
| `not_run` | A driver targets it, but execution was not requested. |
| `untested` | No selected driver explicitly targets it. |

Merely including a helper is not coverage. Reports distinguish `included_by` from
`tested_by`. Filtering cases makes other procedures untested **in that run**; use
the unfiltered snapshot for the overall baseline. A passing driver demonstrates
only its actual cases. See [source-issues.md](source-issues.md) for source-level
limitations found while constructing the tests.

## Adding coverage

1. Add a short program in `drivers/` using public interfaces when possible, so it
   works in full-module mode too. Include relevant edge cases and observable output.
2. Add its ID and the exact specific procedure names it intentionally exercises
   to `cases.json`. Do not list dependencies as tested unless the driver checks them.
   Optional `files` entries provide text fixtures; `dependencies` adds extraction roots.
3. Run that case against gfortran and rfort. Reference failures need investigation,
   rather than being counted as compiler bugs. Undefined source behavior is not an
   output oracle, even if gfortran happens to produce stable output.
4. Run both optimization levels, then refresh the full snapshot after a meaningful
   coverage expansion or compiler fix.

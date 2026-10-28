# Testing

## Regression tests

```sh
cargo build --bin rfort
cargo test --bin rfort --test testsuite -- --test-threads=1
python tests/test_driver.py
python tests/test_module_compilation.py
python tests/test_record_read.py
python tests/test_runtime_read_format.py
python tests/test_named_loops.py
python tests/test_write_controls.py
python tests/test_formatted_implied_do.py
python tests/test_empty_constructors.py
python tests/test_internal_procedures.py
python tests/test_empty_read.py
python tests/test_read_end.py
python tests/test_statement_labels.py
python tests/test_unnamed_procedure_ends.py
python tests/test_declarations_without_colons.py
python tests/test_pointer_association.py
python tests/test_pointer_descriptors.py
python tests/test_recursive_allocatables.py
python tests/test_recursive_arrays.py
python tests/test_deallocate.py
python tests/test_runtime_allocatables.py
python tests/test_heap_allocations.py
python tests/test_allocatable_dummies.py
python tests/test_dimension_attribute.py
python tests/test_internal_read.py
python tests/test_write.py
python tests/test_output_implied_do.py
python tests/test_defined_operators.py
python tests/test_file_io.py
python tests/test_read_comments.py
python tests/test_character_lengths.py
python tests/test_else_if.py
python tests/test_multi_forall.py
python tests/test_forall_functions.py
python tests/test_forward_array_results.py
python tests/test_multirank_array_functions.py
python tests/test_character_allocatable_results.py
python tests/test_array_constructors.py
python tests/test_runtime_constructors.py
python tests/test_bound_inquiries.py
python tests/test_named_formats.py
python tests/test_runtime_formats.py
python tests/test_nonadvancing_output.py
python tests/test_elemental_setters.py
python tests/test_impure_elemental.py
python tests/test_character_elemental.py
python tests/test_character_functions.py
python tests/test_character_array_functions.py
python tests/test_source_diagnostics.py
python tests/test_do_steps.py
python tests/test_where.py
python tests/test_explicit_bounds.py
python tests/test_nested_where.py
python tests/test_masked_reductions.py
python tests/test_dim_reductions.py
python tests/test_optional_procedures.py
python tests/test_optional_elemental.py
python tests/test_character_arrays.py
python tests/test_character_array_parameters.py
python tests/test_scalar_substrings.py
python tests/test_vector_subscripts.py
python tests/test_vector_ranks.py
python tests/test_named_arguments.py
python tests/test_ofort_compat_runner.py
python tests/test_numeric_model.py
python tests/test_bit_intrinsics.py
```

The Python tests use `target/debug/rfort` (with `.exe` on Windows).
`RFORT_TEST_COMPILER` selects another build; the legacy `F90C_TEST_COMPILER`
variable also works. The Rust integration suite defaults to the debug executable
and also accepts `RFORT_TEST_COMPILER`.
Tests require the platform linker/toolchain described above. The regression
runner no longer invokes `git clean`.

The Rust integration suite checks the numbered native fixtures against
[tests/expectations.json](../tests/expectations.json). The commands above include
focused Python checks for the driver and language features; additional
`tests/test_*.py` scripts cover other implemented subsets. Many compare interpreter
execution with native builds at O0 and O2, and also compare with gfortran when it
is available. Python is required for these scripts; gfortran is required for the
external compatibility runner.

On Windows, after building release, also run `python tests/test_batch_sources.py`
to verify the compatibility wrappers and multi-source fixtures.

## Retaining and investigating native failures

Feature tests using the shared `check` or `reject` helper from
`tests/test_deallocate.py` retain unexpected failures under
`target/test-failures`. Set `RFORT_TEST_FAILURE_DIR` to select another directory.
Passing cases are still cleaned up. This includes assertion failures inside
unittest subtests, compilation/link failures, launch errors and timeouts.

Each retained case includes the source, objects, executables and other case
files, plus `failure.json`. That manifest records original and relocated
commands, working directories, timeouts, exit codes, expected results, input,
file hashes, and paths to separate stdout/stderr files for every command.
Temporary outer working directories holding module interfaces or data files
are also copied. Compiler binaries are identified by path and hash; arbitrary
external dependencies are not bundled.

To repeat fresh native O0/O2 builds of a standalone deterministic program:

```bat
python tests/stress_native_output.py tests/programs/native_output/loop.f90 --compiler target/release/rfort.exe --expected-file tests/programs/native_output/loop.txt --iterations 100 --runs-per-build 10 --debug-ir --report target/native-stress/loop.json
```

`--iterations` controls fresh builds; `--runs-per-build` executes the same
unchanged image repeatedly. `--opt-level 0` selects one level; repeat the option
to select several. `--debug-ir` captures the original compiler IR with the build
output. Without `--expected-file`, the interpreter is run once as the output
oracle. `--input-file`, `--compile-timeout` and `--run-timeout` are also available.
The runner uses an isolated working directory; supply a standalone source with
its dependencies already included.

The first failed check remains a failure. The runner then records additional
executions of the exact failing image, re-links the original unchanged object
into a different executable, and records executions of that image. These
diagnostic commands never overwrite the original object/executable or turn a
failure into a pass. `--diagnostic-runs` controls their count. Reports are updated
after every build and the runner exits nonzero on any failure.

An existing executable can be checked without invoking any compiler. For
example, with GCC available, the following control uses no rfortran code:

```bat
gcc tests/programs/native_output/control.c -o target/native-output-control.exe
python tests/stress_native_output.py --executable target/native-output-control.exe --expected-file tests/programs/native_output/control.txt --iterations 1 --runs-per-build 5000 --report target/native-stress/control.json
```

Existing-executable mode launches the original path and retains a copy of the
image on failure. It repeats the image for diagnosis, with no re-link step.

### Intermittent empty output: investigation on October 28, 2026

The Windows investigation captured native processes that returned zero with
empty stdout and stderr. The same unchanged executables produced correct
output immediately before and after the failing executions.

| Case | Observation |
| --- | --- |
| Inherited-constructor loop | Captured at O2; the failing image had already printed correctly seven times. Five diagnostic executions and five executions after re-linking the original object matched. A later 1,000 executions of the original image also matched. |
| Pointer-owner named-kind overloads | Captured at O2; the same image printed correctly six times before the empty result. Original-image and re-linked-image diagnostic executions matched. |
| Allocated target with owned array component | 200 fresh native builds and 2,000 executions matched. |
| Minimal list-directed Fortran output | One empty result in 1,000 executions. |
| Independent GCC `printf` control | One empty result in 1,000 executions; another retained run failed on execution 505. Five immediate diagnostic executions matched. |
| GCC `printf` status control | Execution 270 returned zero with empty output. Its `main` returns 17 if `printf` does not report writing three bytes. Five diagnostic executions matched. |
| Formatted Fortran output, Fortran output followed by FLUSH, Rust `println!` | No failures in 1,000 executions of each control. |
| C control recording execution in a separate file | No failures in 2,000 executions. |

The independent C reproduction means empty output alone is insufficient
evidence of a rfortran code-generation defect. It does not establish that every
observed failure has the same cause. Neither the responsible component nor a
reliable fix has been identified; successful repetitions and the negative
FLUSH control are not proof of a fix. Compiler behavior was left unchanged.

The original affected sources and expected outputs are retained in
`tests/programs/native_output`. Local reports and captured images from this
investigation are under `target/native-stress`, `target/native-output-isolation`
and `target/test-failures`; these generated artifacts are ignored by Git.

Run `python tests/test_failure_artifacts.py` to check the retention and
diagnostic-run behavior, including an injected zero-exit/empty-output failure.

## External util.f90 procedures

Small drivers track progress toward compiling and running `kind.f90 util.f90
xutil.f90`. The suite targets 120 specific procedures in 34 drivers and
inventories all 551 procedures in the current external utility source, leaving
untested procedures visible. It compares native O0/O2 results with gfortran and
retains standalone reproducers under `target/util-repros`.

```bat
python tests/run_util_coverage.py
python tests/run_util_coverage.py --case string_matching --mode both
python tests/test_util_coverage_runner.py
```

The default isolates unchanged procedure bodies with their dependencies;
`--mode module` tests the original full module. `kind.f90`, `util.f90`, gfortran,
and a release compiler build are required. The suite provides differential
checks for the selected procedures. See
[the runner documentation](../tests/util/README.md) and
[the coverage baseline](../tests/util/baseline.json) for scope and results.
The combined build now compiles and executes through `xutil.f90`'s final
intentional `check_all_equal([2,3],...)` stop. This does not imply that every
utility procedure or every output has been validated; isolated drivers remain
the differential checks against gfortran.

## External ofort fixtures

After building release, run the quick 18-case sample or the broader inventory:

```bat
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --report target/ofort-compat.json
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --discover --jobs 4 --report target/ofort-expanded.json
```

Replace `C:\c\ofort` with your ofort checkout. These commands compile standalone
fixtures with rfort and gfortran; they do not run the full ofort Python suite or
modify its checkout. Reports distinguish compilation failures, runtime failures,
wrong output and reference failures. Add `--strict` to return a failing status
when a comparison fails. See [tests/ofort/README.md](../tests/ofort/README.md) for
selection files, discovery exclusions and reproducible checkpoints.

## CHARACTER compatibility inventory

[character-selection.json](../tests/ofort/character-selection.json) selects 84
standalone ofort cases containing CHARACTER declarations or focused string
intrinsics. Run:

```bat
python tests/run_ofort_compat.py --ofort-root C:\c\ofort --selection tests/ofort/character-selection.json --jobs 4 --report target/character-compat.json
```

The [checkpoint](../tests/ofort/character-checkpoint.json) records source hashes,
CHARACTER declaration locations in `util.f90`/`xutil.f90`, and per-case outcomes.
The first CHARACTER batch increased the focused 37-case sample from 6 to 11
passes. At that checkpoint the broader selection had 16 passes, 63 compiler failures and five
reference failures. These broader cases also exercise unsupported derived types,
procedure pointers and I/O; their failures are not all CHARACTER bugs. Major
remaining CHARACTER work includes unbounded runtime-dependent automatic string
storage, deferred-length array dummy arguments and results,
explicit allocation selectors/SOURCE/MOLD, allocatable CHARACTER components,
and internal array files.

## util.f90 progress notes

The full `kind.f90 util.f90 xutil.f90` build now gets past the initialized CHARACTER
local, CHARACTER operator interface, numeric `.notin.` operator and generic
`exclude(shape(...), ...)` call. `SHAPE`, `LBOUND` and `UBOUND` now retain their
INTEGER scalar or rank-one vector types during generic dispatch, and fixed-shape
inquiry vectors can be passed to specialized read-only array functions.
`tests/programs/xexclude.f90` prints `2 4`; `tests/test_generic_inquiries.py` checks dispatch,
keywords, sections, empty extents and initialization diagnostics. The full build
now gets past generic `repeat` resolution: a user generic can fall back to
intrinsic `REPEAT` for scalar CHARACTER/INTEGER arguments, including the intrinsic
keywords `STRING` and `NCOPIES`. Matching user specifics take precedence.
Native `REPEAT` supports constant copy counts, CHARACTER sources and concatenation;
runtime-dependent copy counts currently produce a native compilation error.
`tests/programs/xrepeat_intrinsic.f90` prints `[000]`, and `tests/test_repeat_generic.py` checks
the fallback, keyword association, user overloads and CHARACTER output. The full
build now gets past the constructor-related `set_alloc` dispatch. Constructor
inference flattens scalar and array-valued elements to a rank-one type, checks
type/kind consistency, and infers implied-DO items independently of their bounds.
`tests/test_constructor_inference.py` covers numeric, LOGICAL and CHARACTER items,
nested implied-DOs, typed empty constructors and mixed scalar/array elements.
Read-only numeric/LOGICAL runtime-bound constructor actual arguments now use
runtime storage, including bounds read from input or changed by previous calls.
A compile-time capacity proof is no longer required. The constructor retains
runtime controls and an active length, preserves implied-DO index scope, and
uses checked temporary storage. Nested implied-DOs, runtime positive/negative
strides and empty results are supported. `tests/programs/xset_alloc_implied_do.f90` now compiles
and prints `1 2 3`;
`tests/test_constructor_arguments.py` covers this path in the interpreter and
native code at `-O0` and `-O2`. Generic dispatch now preserves array-section
rank: scalar subscripts remove
axes, while triplets and vector subscripts retain them. Resolver tests cover
ranks through 15 and the rank-three `select_tensor_real` calls in `util.f90`;
`tests/programs/xgeneric_sections.f90` checks generic dispatch after reductions of sections.
`RESHAPE` also retains its source element type and derives result rank from a
scalar-integer shape constructor, independently of runtime extent values.
The full build now passes both `set_alloc` type/rank blockers and currently
stops at a too-many-arguments error for `ivec`.
Full `xutil.f90` execution remains a separate acceptance test.

## External OOP tutorial examples

The optional [OOP compatibility suite](../tests/oop_compat/README.md) compares
checksum-pinned tutorial examples against gfortran in isolated build directories.
External downloads remain under `target`; attribution, license evidence, source
order, and known compiler gaps are recorded in its manifest.

```bat
python tests\run_oop_compat.py --download
python tests\run_oop_compat.py
```

The second command uses the verified cache offline. JSON and Markdown reports
are written to `target/oop-compat/`. Unexpected failures return a nonzero status;
known gaps require a matching phase and diagnostic. See the suite README for
case selection, compiler paths, timeouts, licensing, and the initial checkpoint.

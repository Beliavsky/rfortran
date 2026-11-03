# Small compilation-performance repro

[xslow_array_constructor.f90](../tests/programs/xslow_array_constructor.f90)
initializes a fixed-size array using an implied-DO constructor and prints its
sum. It has no module or external source dependencies.

## Before loop lowering

Measured on Windows x86-64 on 2026-11-03, using the normal release build of
rfort and GNU Fortran 17.0.0 20260510 (experimental):

| Optimization | gfortran compile/link | rfort compile/link | rfort/gfortran |
|---|---:|---:|---:|
| `-O0` | 0.444 s | 6.791 s | 15.3 |
| `-O2` | 0.465 s | 33.180 s | 71.4 |

These are single wall-clock measurements, including process startup and
linking, excluding program execution. All four builds completed and produced
the same numeric output, `50002896.0`. The difference from the mathematical
sum is rounding in the single-precision summation.

The rfort binary was built at 12:55:11 local time, SHA-256
`4623305af8d594c80b1c44669ba53c8059e1ae1b61763396d94ede599d313fb5`.
Measurements used `RFORT_TIME_PASSES=1` for rfort. Local commands, timings and
output are saved in `target/slow-compile-repro/report.json`; pass logs are in
the same ignored directory.

Run from the repository root in CMD:

```cmd
if not exist target\slow-compile-repro mkdir target\slow-compile-repro
gfortran -O2 tests\programs\xslow_array_constructor.f90 -o target\slow-compile-repro\gfortran.exe
target\release\rfort.exe -O 2 tests\programs\xslow_array_constructor.f90 -o target\slow-compile-repro\rfort.exe --time
target\slow-compile-repro\gfortran.exe
target\slow-compile-repro\rfort.exe
```

Repeat with `-O0` for gfortran and `-O 0` for rfort to compare without
optimization. Change `n` to explore scaling. This fixture is not included in
the default regression suite; focused semantic checks live in
`tests/test_large_constructors.py`.

This is a separate bottleneck from the full `fve` group's remaining
`output::prepare_character_views` slowdown. In this repro the `-O2` profile
finishes initialization instrumentation in 4.273 seconds and IR lowering in
0.336 seconds, then spends most of the remaining time after IR lowering.
The much faster `-O0` build points toward backend optimization costs, but
does not identify the exact expensive backend operation.

## Loop-lowering follow-up

Executable assignments to numeric (excluding COMPLEX), LOGICAL, and supported
CHARACTER arrays now retain implied-DO loops with more than 64 iterations
instead of expanding every iteration. They use the existing runtime constructor
lowerer, including temporary storage before destination writes and internal
index variables. The policy also applies inside nested executable scopes.
Small loops, explicitly typed constructors, and constructors required in
constant initializers still expand.
Other expression contexts and the separate COMPLEX/derived-type lowering paths
retain their previous behavior.

Regression coverage includes overlapping assignments, descending and nonunit
strides, nested loops, empty loops, array-valued items, index isolation,
CHARACTER/LOGICAL elements, named kinds, allocatable destinations, shape errors,
constant/SAVE initializers, and compatibility with COMPLEX constructor lowering.

Validation: all 117 constructor tests passed on the final source, covering
the interpreter and native `-O0`/`-O2` execution with gfortran comparisons for
valid cases. Run them with:

```cmd
python -m unittest discover -s tests -p "test_*constructor*.py" -v
```

After rebuilding the normal release compiler, three compile/link runs per
configuration gave these medians (same source and machine, no concurrent builds
or test suites):

| Optimization | rfort before (single run) | rfort after (median) | gfortran after (median) |
|---|---:|---:|---:|
| `-O0` | 6.791 s | 0.296 s | 0.668 s |
| `-O2` | 33.180 s | 0.333 s | 0.513 s |

All outputs matched `50002896.0`. The `-O2` rfort executable decreased from
1,420,288 to 54,272 bytes. These results describe this repro, not a general
compiler performance comparison. Raw samples, commands and the new compiler
hash are in `target/slow-compile-repro/report-after-loops.json`.

`rf.bat tests\programs\xslow_array_constructor.f90 --run --time` also passed
with the rebuilt release, reporting 0.397 seconds for compilation in that run.

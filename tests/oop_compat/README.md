# External OOP tutorial compatibility suite

This optional suite tests complete examples from the resources in the
[Fortran Discourse OOP tutorial list](https://fortran-lang.discourse.group/t/tutorials-on-object-oriented-programming-in-fortran/3052).
It does not change compiler sources or add external tutorials to the normal CI
regression suite.

## Run

Build rfort separately and put gfortran on PATH. From the repository root, in CMD:

```bat
python tests\run_oop_compat.py --list
python tests\run_oop_compat.py --download
```

`--download` fetches missing sources and license notices into
`target/oop-compat/cache`. Each download is checked against a SHA256 recorded in
[manifest.json](manifest.json). Without `--download`, the runner is offline:

```bat
python tests\run_oop_compat.py
python tests\run_oop_compat.py --case "leair-*"
python tests\run_oop_compat.py --compiler target\debug\rfort.exe --case book-type-bound
```

Use `--reference` for a gfortran executable outside PATH. Default compile/run
timeouts are 120/15 seconds; override with `--compile-timeout` and `--run-timeout`.
`--case` accepts globs and can be repeated. Paths containing spaces need quotes.
The Python runner uses only the standard library.

JSON and Markdown reports default to `target/oop-compat/report.json` and
`report.md`; use `--report` to change the destination. Each sweep retains fresh
build directories beside the report. Each case and compiler has separate module
files. Original compiler commands, versions, stdout, stderr, exit codes, timings,
source checksums, and the build directory are recorded. License notices for the
GitHub projects are retained in the build's `notices` directory.

Sources compile in the explicit manifest order. gfortran runs first with
`-O0 -fcheck=all`; rfort uses `-O0`. All nine initial cases have deterministic
output. The shared ofort comparison helper compares whitespace-separated values,
allows real roundoff (relative tolerance 2e-6, absolute tolerance 1e-7), normalizes
logical spelling, and compares integer values exactly. It does not check output
layout. All programs run with empty standard input.

## Sources, attribution, and licensing

External source files are downloaded under ignored `target`, not tracked or
relicensed as compiler code. The manifest records source URLs, pinned revisions
where available, SHA256, attribution, and license evidence. Mutable SourceForge
URLs are content-pinned by SHA256: an upstream edit fails verification until
reviewed. A corrupt cache entry also fails; it is not silently replaced.

| Resource | Selection | License evidence |
|---|---|---|
| Mark Leair, *Object-Oriented Programming in Fortran 2003*, Part 1, mirrored by n-s-k | First module code block in sections 5, 7, and 9; unchanged, with separate rfortran drivers | No redistribution license identified for the article; download only. [Pinned mirror](https://gist.githubusercontent.com/n-s-k/522f2669979ed6d0582b8e80cf6c95fd/raw/be14ed9126bd40873c00a8c1d702ce77dda53f54/OOP_F2003_Part_1.md) |
| Arjen Markus, FortranCon 2020 workshop | `moving_average.f90`, `abstract_framework.f90`, unmodified complete programs | [MIT repository license](https://github.com/fortrancon/workshop2020/blob/4bf1740a7cc40e4cf20209a7dd984825e094e6ba/LICENSE) |
| Jonas Lindemann and Ola Dahlblom, *Modern Fortran in Science and Technology* | `mytype_module.f90` with our driver; complete `polymorphism` example, modules preceding main program | [GPL v3 repository license](https://github.com/jonaslindemann/modern_fortran_book/blob/4ffe5ec194c50f6fbd222928bfa54c878ca842c3/LICENSE); downloaded tests retain their own license |
| Arjen Markus, *Modern Fortran in Practice*, chapter 11 | `abstract_point.f90`, `classes_geom.f90`, unmodified complete programs | [CC BY 3.0, stated on the examples index](https://flibs.sourceforge.net/examples_modern_fortran.html); notices also appear in the source files |

The small drivers under `drivers` are rfortran test code. Leair's tutorial modules
are extracted at run time; the article itself is verified before extraction.
Downloaded examples and their executables are not release artifacts. Do not
copy the cache into the repository under the compiler's license.

Other resources from the supplied list remain candidates, not tested coverage:

- Reinhold Bader, [Object-Oriented Features in Fortran 2003](https://www.elch.chem.msu.ru/tch/group/FortranBooks/Fortran_Object_Oriented.pdf): select complete examples from the slides and check redistribution terms.
- Katherine Holcomb's tutorial and the PRACE advanced Fortran slides: locate stable downloadable sources through the Discourse index before selecting cases.
- Steve Lionel, [Doctor Fortran in Not My TYPE](https://stevelionel.com/drfortran/2020/06/30/doctor-fortran-in-not-my-type/): select snippets and supply explicit drivers; check terms.
- Tiziano Mueller's constructors/patterns presentation, linked from the Discourse index: inspect complete companion examples and their license before adding.
- Leair Parts 2-4: linked lists, parameterized types, and defined I/O remain future coverage. Part 2 companion sources contain restrictive NVIDIA notices; they are not vendored here. The old PGI article URLs redirect, so stable mirrors/archives are needed.

## Checkpoint (2026-10-27)

All nine examples compile and run with gfortran. Six match rfort's output:
Leair sections 5, 7, and 9, the book's `mytype` and polymorphism examples, and
the FortranCon moving-average example.

The three current compiler gaps are recorded individually in the manifest:

| Case | First blocker |
|---|---|
| `workshop-abstract-framework` | SELECT CASE list parsing |
| `markus-abstract_point` | Binding dispatch through a polymorphic pointer |
| `markus-classes_geom` | DIMENSION attribute on a local derived-type array |

Leair section 9 now passes unchanged: PRIVATE component/binding declarations,
accessibility checks, the procedure name `print`, and the constructor result
using its function name are supported. The focused regression
`tests/programs/xprivate_constructor_result.f90` prints `7` with both compilers.
`tests/test_module_function_results.py` covers function-name and explicit RESULT
variables, typed headers, import aliases/re-export, forwarding, intrinsic array
results and separate module compilation.

The moving-average example also passes unchanged. IEEE_VALUE keeps its mold's
type, kind and elemental shape without reading its undefined stored value;
`tests/test_ieee_value_mold.py` checks scalar/array molds, runtime allocation,
shape conformance, and preservation of genuine initialization diagnostics.

The book's polymorphism example now passes unchanged, including intrinsic
TYPE IS guards for CLASS(*), derived CLASS IS dispatch, and CLASS DEFAULT.
`tests/test_select_type_intrinsic.py` also covers kind and CHARACTER length
matching, writable aliases, forwarding, and separate module compilation.

These are first failures, not a claim that fixing one will make its entire
example pass. The suite deliberately preserves upstream code so it can expose
interactions that focused regression tests miss. A gfortran failure is reported
as a reference failure, rather than automatically blamed on rfort.

Known gaps are accepted only at the recorded phase with the recorded diagnostic
signature. A different error, crash without that signature, timeout, source
error, launch failure, reference failure, or output mismatch makes the runner
exit nonzero. A case that starts passing is reported as `newly_supported`;
remove its `known_failure` entry after review. Successful ordinary cases are
`matched_reference`. Accepted gaps keep this optional sweep useful without
turning every currently unsupported tutorial into a CI regression.

## Extend and test the runner

Add each resource with attribution, license evidence, URL/revision and SHA256;
add case files in dependency order. Choose a complete deterministic program,
or provide a small driver. Incomplete snippets and random-output programs need
explicit treatment before inclusion, rather than an unrestricted ignore rule.
Keep downloads and build products under `target`.

Offline harness tests, independent of compilers and network:

```bat
python -m unittest discover -s tests -p test_oop_compat_runner.py
```

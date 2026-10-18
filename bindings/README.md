# Fortran-to-Rust binding generator prototype

This standalone tool generates **Fortran wrappers and Rust bindings**, leaving the
Fortran implementation in Fortran. It does not translate algorithms into Rust.
Everything needed for the prototype is under `bindings/`; build output goes under
`target/`. It neither imports nor edits the rfortran frontend, compiler, transpiler,
interpreter, root Cargo files, or existing tests. No rfort rebuild is needed.

The first version takes a reviewed JSON interface description. It does **not** yet
extract declarations from Fortran source. This separates the binding ABI from the
frontend being edited elsewhere; a future frontend adapter can emit the same
descriptions after resolving types, kinds, ranks, intents, and module interfaces.

## Run on Windows

Requires Python 3.10+, gfortran, and Rust 1.82+ with a working native linker. Both
native compilers must target the same architecture. No Python packages, Cargo
dependencies, or network downloads are required.

```cmd
python bindings\run_demo.py
python bindings\run_demo.py --example rnorm
python -m unittest discover -s bindings/tests -v
```

The first command builds a Fortran DLL and a Rust executable, then verifies calls
with `f32`, `f64`, `i32`, `i64`, scalar outputs, input/output arrays, zero-length
arrays, and mismatched-length rejection. The second calls the **existing,
unchanged `rnorm.f90`** from Rust and prints sample statistics. It makes all calls
on one thread because that Fortran routine has saved state. Statistics are
illustrative, not a probabilistic pass/fail test.

The demo has been tested with x86-64 Windows MSVC Rust and gfortran. Windows Rust
uses `raw-dylib` linking, so no separately generated MSVC import library is needed.
The runner also contains Linux/macOS shared-library commands, but those platforms
have not been verified here. The runner adds the compiler's bin directory to its
child process PATH; a different Fortran distribution may require additional runtime
DLL directories. It does not change the user's persistent environment.

Unit tests run without native compilers; integration tests are skipped if gfortran
or rustc is absent. The rnorm integration test is also skipped if the root example
is absent (root scratch `.f90` files are normally ignored by Git).

## Generate bindings for a library

```cmd
python bindings\generate.py bindings\examples\demo.json --out-dir target\my-bindings
```

This writes `bindings.f90` and `bindings.rs`. Compile the original Fortran module
and `bindings.f90` together using the same Fortran compiler and compatible flags;
the wrapper uses the module's explicit interface. Link the generated Rust module
to that shared library. `run_demo.py` is an executable example of the build steps.
Generated files are overwritten when regenerated; use a dedicated output directory.

A minimal manifest:

```json
{
  "library": "my_library",
  "procedures": [
    {
      "module": "math_mod",
      "name": "scale_values",
      "kind": "subroutine",
      "args": [
        {"name": "x", "type": "f64", "intent": "inout", "rank": 1},
        {"name": "factor", "type": "f64", "intent": "in"}
      ]
    }
  ]
}
```

The original procedure could be:

```fortran
module math_mod
   use iso_c_binding, only: c_double
   implicit none
contains
   subroutine scale_values(x, factor)
      real(c_double), intent(inout) :: x(:)
      real(c_double), intent(in) :: factor
      x = factor*x
   end subroutine scale_values
end module math_mod
```

The Rust-facing interface is:

```rust
pub unsafe fn scale_values(x: &mut [f64], factor: f64)
```

The generated Fortran wrapper is `BIND(C)`. Its array argument has an explicit
extent passed alongside a data pointer. The Fortran compiler constructs the
descriptor needed to call the original assumed-shape procedure; Rust does not
depend on that compiler's internal descriptor layout. Input scalars cross the
boundary by value, while output/inout scalars cross by pointer.

## Interface contract

| Manifest field | Meaning |
| --- | --- |
| `library` | Shared library base name, without prefix or extension; up to 24 characters |
| `module`, `name` | Existing accessible module procedure |
| `rust_name` | Optional public Rust alias, useful for duplicate names across modules |
| `kind` | `function` or `subroutine` |
| `result` | Required scalar type for a function; absent for a subroutine |
| `args` | Dummy arguments with their original names; generated calls use keywords |
| `type` | `f32`, `f64`, `i32`, or `i64` |
| `intent` | `in`, `out`, or `inout` |
| `rank` | Omitted/0 for scalars, 1 for contiguous assumed-shape array dummies |
| `same_length` | Optional groups such as `[["x", "y", "z"]]`; checked before the native call |

Types map to `c_float`, `c_double`, `c_int32_t`, and `c_int64_t`, respectively.
The generator does not guess what a default INTEGER, REAL, or user-defined kind
means. Names are normalized to lowercase. Rust keywords and the `bg_` helper prefix
are reserved; array argument names are limited to 58 characters. Unknown fields,
unsupported ranks/types, missing intents, and name collisions produce errors.

The manifest is an assertion about the source, not a source-validation report.
Compiling the wrapper catches many incorrect interfaces, including incompatible
argument types and ranks. It cannot establish all extent relationships, retained
pointers, global side effects, or semantic preconditions. Numeric function results
are assigned to the declared wrapper result kind using ordinary Fortran assignment;
review that kind to avoid unintended conversion or precision loss.

## Why the Rust functions are unsafe

Generated Rust exposes slices/references and checks declared equal-length groups,
but deliberately keeps every native call `unsafe`. Callers must review the
manifest and honor the original procedure's preconditions. Native code must not
retain borrowed addresses, access overlapping mutable storage, or race on module
variables/SAVE state. Fortran STOP/ERROR STOP and runtime failures can terminate
the Rust process; they are not translated into Rust errors or caught by
`catch_unwind`. The library and its runtime must remain available when called.

This prototype is not a sandbox. Building or calling Fortran executes native code.

## Current limits and next steps

Supported: public module functions with numeric scalar results, module subroutines,
numeric scalars, and contiguous rank-one assumed-shape arrays, including empty
arrays when the target permits them. Callers provide initialized Rust storage for
output arguments. No buffers are allocated or freed across compiler runtimes.

Not yet supported: source interface discovery, external procedures without module
interfaces, multidimensional arrays, array results, CHARACTER, LOGICAL, COMPLEX,
OPTIONAL, allocatable or pointer dummies, callbacks, and derived-type arguments.
Do not describe such arguments as ordinary arrays/scalars to bypass these limits.

Useful next steps are automatic interface discovery in a separate adapter, then
multidimensional arrays with explicit shape checks. Derived types such as the
mixture parameter objects can later remain inside a Fortran adapter while Rust
supplies numeric buffers, or be represented by managed opaque handles. Those
features do not require changing the numerical Fortran algorithms.

Technical references:

- [GNU Fortran: interoperable procedures and VALUE](https://gcc.gnu.org/onlinedocs/gfortran/Interoperable-Subroutines-and-Functions.html)
- [GNU Fortran: array descriptors and further C interoperability](https://gcc.gnu.org/onlinedocs/gfortran/Further-Interoperability-of-Fortran-with-C.html)
- [Rust: external blocks and library linkage](https://doc.rust-lang.org/reference/items/external-blocks.html)

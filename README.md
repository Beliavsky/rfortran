# rfortran - Fortran compiler in Rust

The compiler command is `rfort`. This project is a fork of
[SirPigari/f90c](https://github.com/SirPigari/f90c).

`rfort` is a single executable Fortran compiler written in Rust. It can compile, link, and run Fortran 90 sources, supporting object files, optimizations, warnings, and multiple subcommands.

---

## Installation

Build the compiler from this checkout:

```bash
cargo build --release --bin rfort
```

On Windows this creates `target/release/rfort.exe`. Add this repository to your
`PATH` to use `rfort.bat`, or invoke it as `rfort.bat` from this directory.
On other systems the executable is `target/release/rfort`.

---

## Usage

The general command structure is:

```bash
rfort [subcommand] [links.o (.obj)] [inputs.f90] [options]
```

If no subcommand is provided, all `.f90` inputs are compiled into an executable, optionally linking with `.o` (`.obj`) object files.

### Windows batch wrapper

```bat
rfort.bat m1.f90 m2.f90 main.f90
```

`f90c.bat` remains as a compatibility entry point and delegates to `rfort.bat`.
Both use the same compiler, arguments, and exit status.

List modules in dependency order, followed by the main program: if `m2` uses
`m1`, put `m1.f90` first. This creates `main.exe` and `main.obj` alongside the
last source file. Quote paths containing spaces. Successful builds are silent;
the default optimization level remains `-O2`.

For multiple inputs, the wrapper uses the accompanying `f90c-sources.ps1`
helper to combine the sources and compile them together. This makes module
constants, procedure definitions, derived types, and operator interfaces
available to the compiler. It does not create or reuse separately compiled
module files. The compiler's existing language restrictions still apply.

The generated `main.f90c-sources.f90` is retained next to the executable and
ignored by Git. Diagnostics refer to its line numbers; `! Source:` comments
identify the original files. A subsequent build refreshes this generated file.
Single-source commands continue to compile the original file directly.

Run the wrapper integration tests with `python tests/test_batch_sources.py`.
They require Windows, a built compiler, and the linker/library setup specified
in `rfort.bat`.

---

## Quick Reference

| Subcommand | Description |
|------------|-------------|
| `lex` | Dump tokens of a `.f90` source file |
| `parse` | Dump AST of a `.f90` source file |
| `check` | Run semantic checks only |
| `build` | Compile a `.f90` source into an executable |
| `link` | Link `.o` (`.obj`) object files and `.f90` sources into an executable |
| `emitobj` | Emit a `.f90` source as a `.o` (`.obj`) object file |
| `run` | Parse, check, and execute a `.f90` source file via JIT |
| `help` | Show help for `rfort` or subcommands |

### Options

| Flag | Description |
|------|-------------|
| `-o`, `--out <path>` | Output executable or object path. Defaults derived from the first source |
| `--Wall` | Enable all warnings |
| `--Werror` | Treat warnings as errors |
| `--lto` | Enable Link Time Optimization (LTO) for better performance |
| `-O`, `--opt-level <LEVEL>` | Set optimization level: `0`, `1`, `2`, `3`, `s`, `z`, `S`, `Z` (default `2`) |
| `-q`, `--quiet` | Suppress all output except errors |
| `-h`, `--help` | Show help message |
| `-v`, `--version` | Show version information |

---

## Subcommands Details

### `lex`

```bash
rfort lex input.f90
```

- `input.f90` – Input Fortran 90 source file

---

### `parse`

```bash
rfort parse input.f90
```

- `input.f90` – Input Fortran 90 source file

---

### `check`

```bash
rfort check input.f90
```

- `input.f90` – Input Fortran 90 source file

---

### `build`

Compile a `.f90` source into an **executable**.

```bash
rfort build input.f90 [options]
```

Options:

- `-o`, `--out <path>` – Output path for the executable  
- `--run` – Run after build

---

### `link`

Link `.o` (`.obj`) object files and `.f90` sources into an executable.

```bash
rfort link links.o inputs.f90 [options]
```

Options:

- `-o`, `--out <path>` – Output executable path  

> Sources will be compiled first into `.o` (`.obj`) object files if included

---

### `emitobj`

Emit a `.f90` source as a `.o` (`.obj`) object file to a specific path.

```bash
rfort emitobj input.f90 -o output.o
```

- `input.f90` – Input Fortran 90 source file  
- `output.o` – Output object file path (`.obj` on Windows)

---

### `run`

Parse, check, and execute a `.f90` source file via JIT.

```bash
rfort run input.f90
```

- `input.f90` – Input Fortran 90 source file

---

### `help`

Show help for `rfort` or subcommands.

```bash
rfort help
```

---

## Examples

Compile a single `.f90` source into an executable:

```bash
rfort main.f90
```

This produces an executable derived from the first source name.

---

Compile multiple `.f90` sources directly:

```bash
rfort main.f90 file1.f90 file2.f90
```

Each file is compiled **separately** into its own executable.  
This does **not** link them together.

---

Build reusable object files (modules only, no `program`):

```bash
rfort lib1.f90
rfort utils.f90
```

This produces `lib1.o` (`lib1.obj`) and `utils.o` (`utils.obj`).

---

Link a main program with object files:

```bash
rfort main.f90 lib1.o utils.o
```

This compiles `main.f90` and links it with the object files into a single executable.

---

Build an executable with a custom name:

```bash
rfort build main.f90 -o program
```

This compiles `main.f90` into an executable named `program`.

---

Enable all warnings and treat them as errors:

```bash
rfort main.f90 --Wall --Werror
```

---

Run a `.f90` program with JIT:

```bash
rfort run main.f90
```

---

## License

The combined compiler is licensed under **[GPLv3](LICENSE)**. Original Rust
modules added in this fork and listed in [LICENSING.md](LICENSING.md) are also
available individually under the **[MIT license](LICENSE-MIT)**. Their source
files carry MIT SPDX headers. Upstream code remains subject to GPLv3.

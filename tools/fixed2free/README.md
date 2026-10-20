# fixed2free

Convert Fortran fixed source form to free source form.

`fixed2free` is a standalone package with its own workspace. The rfortran
compiler also uses its library to read `.f`, `.for`, `.ftn` and `.f77` files:
line `n` of the output is line `n` of the input, so diagnostics point at the
original line, and `Converted::map` gives the input byte offset of every output
byte. Each warning has a `WarningKind`, so a caller can choose which to show.

## Build and use

```
cd tools/fixed2free
cargo build --release
target/release/fixed2free old.f            # writes old.f90 beside it
target/release/fixed2free -o - old.f       # writes standard output
target/release/fixed2free src/*.f          # converts each file
```

Each output is written beside its input with the extension `.f90`, or `.F90`
for `.F`, `.FOR` and `.FPP` sources, which are preprocessed. Existing files are
not overwritten without `--force`.

| Option | Meaning |
|---|---|
| `-o FILE` | output for a single input; `-` writes standard output |
| `--suffix EXT` | extension of the outputs |
| `--force` | overwrite existing outputs |
| `--line-length N\|none` | last column of the statement field (default 72) |
| `--debug-lines` | compile lines with `D` in column 1 instead of commenting them |
| `--hollerith format\|all\|none` | which Hollerith constants become character literals (default `format`) |
| `-q`, `--quiet` | do not print warnings |

## What it does

- Comment lines (`C`, `c`, `*` or `!` in column 1, or `!` as the first
  nonblank character outside column 6) become `!` comments; `D` debug lines
  become comments unless `--debug-lines` is given.
- Labels and statement text keep their columns, so the output looks like the
  input. Tab format (a tab in the label field, and a tab followed by a nonzero
  digit for a continuation) is expanded to columns.
- A continuation line starts with `&` in column 6, and the line it continues
  gets `&` after its code, before any inline comment. Comment lines between
  them are kept.
- Text after the last column (72 by default) is dropped, as fixed form
  specifies; a warning counts the lines that had any, in case they are code
  rather than sequence numbers.
- A character literal or Hollerith constant continued onto the next line keeps
  the blanks up to the last column, as fixed form specifies.
- Hollerith constants in FORMAT statements, such as `5HHELLO`, become character
  literals (`'HELLO'`). Elsewhere they are kept by default, because a Hollerith
  constant that initializes or is passed as a numeric value means something
  else as a character literal; `--hollerith all` converts them too.
- Fixed form ignores blanks, so names on either side of a continuation join.
  When the continued line fills its last column, its last name runs on into the
  next line; otherwise a blank is inserted so that, for example, `REAL` and `X`
  on two lines stay apart.

Warnings report statements that start before column 7, continuation lines
without a statement, labels on continuation lines, unterminated character
literals, kept Hollerith constants and output lines longer than 132 columns.

## Not handled

Fixed form ignores blanks everywhere, so code written without them, such as
`DO10I=1,5` or `GOTO10` or `REALX`, is valid fixed form but not free form.
Code with conventional spacing, which is nearly all of it, converts as it is.

## Checking against gfortran

`check_gfortran.py` converts each fixed-form file, compiles the original as
fixed form and the result as free form with `gfortran -fdump-tree-original`,
and compares the dumps; equal dumps mean gfortran read both as the same
program.

```
python check_gfortran.py path/to/lapack/SRC path/to/other.f
```

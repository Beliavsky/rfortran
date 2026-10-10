# Source licensing

This compiler is a fork of [SirPigari/f90c](https://github.com/SirPigari/f90c).
The combined compiler is distributed under GPLv3; see [LICENSE](LICENSE).
Upstream source and files containing upstream source remain subject to GPLv3.

The following original Rust modules and C runtime adapter added in this fork are additionally
available under the [MIT license](LICENSE-MIT). Each carries an
`SPDX-License-Identifier: MIT` header and a copyright notice for Beliavsky,
the author recorded in this repository's Git history.

- `src/array_calls.rs`
- `src/function_arrays.rs`
- `src/array_bounds.rs`
- `src/derived.rs`
- `src/driver.rs`
- `src/file_io.rs`
- `src/codegen/file_io.rs`
- `src/codegen/characters.rs`
- `src/elemental.rs`
- `src/formatting.rs`
- `src/format_spec.rs`
- `src/native_format.rs`
- `src/imports.rs`
- `src/initialized.rs`
- `src/input.rs`
- `src/intrinsics.rs`
- `src/inquiries.rs`
- `src/libs/portable_runtime.c`
- `src/multidim.rs`
- `src/operators.rs`
- `src/output.rs`
- `src/parameters.rs`
- `src/purity.rs`
- `src/do_steps.rs`
- `src/reductions.rs`
- `src/sections.rs`
- `src/subroutines.rs`

The MIT permission applies to these listed modules individually. It does not
relicense upstream code, the upstream portions of modified files, or the
combined compiler under MIT. Using the modules with the GPL-covered compiler
requires compliance with the GPL for the combined work. Preserve the MIT
copyright and permission notice when distributing the modules or substantial
portions of them. Third-party dependencies and bundled tools retain their own
licenses.

## Comparison used for these notices

On 2026-10-10, the original 16 Rust modules were compared with the saved upstream source in
`C:\rust\public_domain\github\f90c`. None of their paths exists in that
baseline, and their Git histories record Beliavsky as their author. Comparison
of source contents found no substantial copied blocks in those modules.
This is a source-provenance review, not a guarantee about all possible
copyright claims or third-party sources. The GPL license file was preserved
unchanged. Existing upstream files and additions within those mixed files were
not given whole-file MIT notices.

The shared driver and portable runtime adapter were added under MIT during the
native-driver redesign after the baseline comparison described above.

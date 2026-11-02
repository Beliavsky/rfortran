# rfortran: an experimental Fortran compiler and interpreter in Rust

Draft: post after the browser deployment is current and the Windows prerelease
has been reviewed and published. Replace the release link if the version changes.

---

I've been developing **rfortran**, an experimental Fortran compiler, interpreter
and REPL written in Rust, forked from [SirPigari/f90c](https://github.com/SirPigari/f90c).

You can now try the interpreter and REPL directly in a browser, with no installation:

**https://beliavsky.github.io/rfortran/**

The project is intended partly as a place to experiment with features from
recent Fortran standards and to build small, reproducible language tests.
The native compiler generates code using Cranelift.

A Windows x64 prerelease includes the compiler, interpreter and REPL in one
executable. Interpretation does not require Rust or a native build toolchain;
native compilation needs the tools described in the download's installation notes.

- Source and documentation: https://github.com/Beliavsky/rfortran
- Windows prerelease: https://github.com/Beliavsky/rfortran/releases/tag/v0.1.0-alpha.1
- Feature status: https://github.com/Beliavsky/rfortran/blob/main/docs/feature-status.md

Language support is incomplete, and the compiler, interpreter and REPL have
different limitations. This is an experimental project, not a production
alternative to gfortran or ifx. Small bug reports, tests, and feedback on
features people would like to experiment with would be welcome.

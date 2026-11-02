Experimental rfortran compiler, interpreter and REPL, written in Rust and forked
from [SirPigari/f90c](https://github.com/SirPigari/f90c).

Try the browser interpreter: **https://beliavsky.github.io/rfortran/**

The Windows x64 ZIP contains one `rfort.exe`, examples, installation notes,
feature status and license notices. Rust is not needed to use the download.

```cmd
rfort run examples\hello.f90
rfort repl
rfort examples\hello.f90 -o hello.exe --run
```

Interpretation and the REPL do not need a development toolchain. Native
compilation needs LLVM's linker, Visual Studio C++ Build Tools and the Windows
SDK; these are not bundled. See INSTALL.md in the archive.

The package is tested after extraction, including interpretation, supplied
input, the REPL and native execution at -O0 and -O2. Its smoke-test report is
attached. SHA256SUMS.txt covers the binary ZIP, report and matching source archive.

This is a prerelease with incomplete language support. The native compiler,
interpreter and REPL have different limits; this is not a replacement for an
established production compiler. Windows x64 is the first binary release.
Linux/macOS native builds are undergoing separate validation and are not
advertised as supported release downloads yet.

See the [feature-status document](https://github.com/Beliavsky/rfortran/blob/main/docs/feature-status.md).
Please report bugs with the version, OS, command, small reproducer, expected
result and actual result.

The combined project is GPLv3; upstream attribution and individually applicable
MIT notices are included in the distribution.

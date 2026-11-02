Experimental rfortran compiler, interpreter and REPL, written in Rust and forked
from [SirPigari/f90c](https://github.com/SirPigari/f90c).

Try the browser interpreter: **https://beliavsky.github.io/rfortran/**

Downloads are a Windows x64 ZIP, Linux x86-64 tar.gz and macOS Apple Silicon
(arm64) tar.gz. Each contains one
`rfort` executable, examples, installation notes, feature status and license
notices. Rust is not needed to use these downloads.

```cmd
rfort run examples\hello.f90
rfort repl
rfort examples\hello.f90 -o hello.exe --run
```

Interpretation and the REPL do not need a development toolchain. Native
compilation needs LLVM's linker, Visual Studio C++ Build Tools and the Windows
SDK on Windows; these are not bundled. On Linux, native compilation requires
a system C compiler, linker and development libraries (for example Ubuntu's
`build-essential`). On macOS, native compilation requires Apple's Command Line
Tools or Xcode. See INSTALL.md in the corresponding archive.

The Linux archive targets x86-64 with glibc 2.35 or newer and is checked on Ubuntu
22.04 and 24.04. It does not support musl/Alpine, 32-bit x86 or ARM. The macOS
archive targets Apple Silicon with macOS 14 or newer and is checked on macOS
14 and 15; Intel Macs are not included. On Linux or macOS:

```sh
./rfort run examples/hello.f90
./rfort repl
./rfort examples/hello.f90 -o hello --run
```

The package is tested after extraction, including interpretation, supplied
input, the REPL, native execution at -O0 and -O2, C interoperability and separate
module compilation. Linux and macOS archive checks run in fresh jobs on both
specified OS versions. Smoke-test reports are attached. SHA256SUMS.txt covers all binary
archives, reports and the matching source archive.

This is a prerelease with incomplete language support. The native compiler,
interpreter and REPL have different limits; this is not a replacement for an
established production compiler. Windows has received more extensive local
testing; Linux validation does not establish compatibility with every Linux
distribution. The macOS executable is ad-hoc signed, without Developer ID signing
or notarization; browser-downloaded copies may trigger Gatekeeper prompts.
Installation notes explain this limitation; CI does not reproduce those prompts.

See the [feature-status document](https://github.com/Beliavsky/rfortran/blob/main/docs/feature-status.md).
Please report bugs with the version, OS, command, small reproducer, expected
result and actual result.

The combined project is GPLv3; upstream attribution and individually applicable
MIT notices are included in the distribution.

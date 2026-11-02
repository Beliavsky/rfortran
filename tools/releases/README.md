# Native prerelease packaging

The prerelease workflow packages **Windows x64, Linux x86-64 and macOS Apple
Silicon (arm64)**. Each executable contains the compiler, interpreter and REPL;
there are no separate interpreter downloads. Linux archives must pass extracted
archive checks on Ubuntu 22.04 and 24.04; macOS archives must pass on macOS 14
and 15 Apple Silicon. All platform checks must succeed before the draft release
is assembled.

## Build and preview locally

Use Python 3.11 or later, Rust's x86_64-pc-windows-msvc toolchain, LLVM with an
LLD linker on PATH, Visual Studio C++ Build Tools and the Windows SDK. From CMD:

```cmd
set "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS=-C target-feature=+crt-static"
cargo build --release --locked --bin rfort --target x86_64-pc-windows-msvc --target-dir target/release-candidates
python tools\releases\test_package.py
python tools\releases\package_windows.py --binary target\release-candidates\x86_64-pc-windows-msvc\release\rfort.exe --allow-dirty
```

The default build does not embed objdump. Developers who deliberately want the
old bundled disassembler can build with `--features bundled-objdump`; it is
not part of release builds or source archives (use a full Git checkout for that
optional feature). Standard assembly output can use an external
objdump or dumpbin. Compilation/interpreting do not require a disassembler.

Packaging writes `target/release-assets`. It rejects non-system DLL dependencies,
collects dependency license texts and metadata, creates a ZIP, extracts it into
an unrelated temporary path containing spaces, then tests the extracted binary.
Interpreter/REPL tests run with development tools removed from PATH. Native
compilation and execution are checked at -O0 and -O2 with the full toolchain.
Version, help, arrays, modules/functions, standard input, C interoperability and
separate module compilation/linking are exercised.

Uncommitted or untracked files make a build dirty. Packaging normally requires
a clean checkout. `--allow-dirty` creates a ZIP whose name contains PREVIEW;
it is only a local check, not an official release. No source archive is generated
for a dirty preview because a Git archive would not contain those changes.

Clean packaging creates:

- `rfortran-<version>-windows-x86_64.zip`
- `rfortran-<version>-windows-x86_64-smoke.json`
- `rfortran-<version>-source.tar.gz`, including the committed Cargo.lock
- `SHA256SUMS.txt`, covering those three assets

The source archive and source revision in BUILD.json must match the binary.
Packages include GPL/MIT project notices and the selected Cargo dependencies'
license texts. Missing texts fail packaging. Workspace members that omit texts
can use a peer crate's repository-wide license only if repository and SPDX match
(or its Cargo VCS commit and SPDX match, covering missing/subdirectory repository URLs);
the origin is recorded in THIRD-PARTY-LICENSES/index.json. License selection uses
each platform's Cargo target; Linux and macOS do not inherit the Windows
dependency list.

## Linux x86-64 packaging and verification

The workflow builds `x86_64-unknown-linux-gnu` on Ubuntu 22.04. Packaging inspects
the ELF header, loader, shared-library dependencies and glibc symbol versions,
rejecting non-system libraries, RPATH/RUNPATH and requirements newer than glibc
2.35. The advertised baseline is Linux x86-64 with glibc 2.35 or newer; this is
not a claim to support every distribution. musl/Alpine, 32-bit x86 and ARM are
not targets of this archive.

On a Linux development host with Rust, Python 3.11+, `build-essential` and
`binutils`, the equivalent build/package commands are:

```sh
cargo build --release --locked --bin rfort --target x86_64-unknown-linux-gnu --target-dir target/release-candidates
python tools/releases/test_package.py
python tools/releases/package_linux.py build --binary target/release-candidates/x86_64-unknown-linux-gnu/release/rfort
```

This creates `rfortran-<version>-linux-x86_64.tar.gz` and `SHA256SUMS-linux.txt`.
The archive preserves executable permissions and includes examples, installation
notes, feature documentation, BUILD.json and dependency license texts. It does
not bundle a C compiler or linker; native compilation needs the system C
development toolchain. See [INSTALL-linux.md](INSTALL-linux.md).

Separate GitHub jobs download the archive onto Ubuntu 22.04 and 24.04, verify
its checksum and source revision, extract into a temporary path containing
spaces, and run the shared smoke checks outside the repository. No compiler
build is performed in these verification jobs. Both must pass before the draft
release can be assembled. To run the archive checks manually on Linux:

```sh
python tools/releases/package_linux.py verify --archive target/release-assets/rfortran-0.1.0-alpha.1-linux-x86_64.tar.gz --report target/release-assets/linux-smoke.json
```

`build --allow-dirty` is available for inspecting local PREVIEW packages, but
release verification deliberately rejects dirty packages. Native Linux execution
cannot be validated by running the packaging unit tests on Windows alone.

## macOS Apple Silicon packaging and verification

The workflow builds `aarch64-apple-darwin` on an Apple Silicon `macos-14` runner
with `MACOSX_DEPLOYMENT_TARGET=14.0`. It explicitly verifies the host architecture
and applies an ad-hoc code signature to the final executable after Cargo's
release stripping. No Apple credentials are needed for this signature.

Packaging inspects the Mach-O header, deployment target and dynamic libraries,
rejecting Intel/arm64e/universal binaries, non-system libraries, LC_RPATH and
minimum macOS versions newer than 14.0. It verifies the embedded code signature.
The archive is `rfortran-<version>-macos-arm64.tar.gz`, with a separate
`SHA256SUMS-macos.txt` before release assembly. It includes target-specific
license texts and [installation instructions](INSTALL-macos.md).

Linux and macOS share archive staging, permission preservation, extraction and
verification in `package_unix.py`; their dependency audits remain specific to
each format. On an Apple Silicon development Mac with Rust, Python 3.11+ and
Apple's Command Line Tools:

```sh
export MACOSX_DEPLOYMENT_TARGET=14.0
cargo build --release --locked --bin rfort --target aarch64-apple-darwin --target-dir target/release-candidates
codesign --force --sign - target/release-candidates/aarch64-apple-darwin/release/rfort
python tools/releases/test_package.py
python tools/releases/package_macos.py build --binary target/release-candidates/aarch64-apple-darwin/release/rfort
python tools/releases/package_macos.py verify --archive target/release-assets/rfortran-0.1.0-alpha.1-macos-arm64.tar.gz --report target/release-assets/macos-smoke.json
```

Fresh macOS 14 and 15 jobs download the archive, check its checksum, source
revision, architecture, libraries and signature, then run the shared 16 smoke
checks outside the checkout in a path containing spaces. No compiler build is
performed in these verification jobs.

The package is **not Developer ID signed or notarized**. Browser download
quarantine and Gatekeeper prompts are not reproduced by these CI checks;
INSTALL.md explains that limitation and links to Apple's guidance. Developer
ID signing and notarization would require a separate signing setup. This release
does not include Intel macOS or promise compatibility with macOS before 14.

## Create a draft prerelease

Commit all release changes, including Cargo.toml, Cargo.lock, release scripts,
examples, workflows and documentation. The tag version must match Cargo.toml.

```cmd
git push origin main
git tag -a v0.1.0-alpha.1 -m "Experimental Windows, Linux and Apple Silicon prerelease"
git push origin v0.1.0-alpha.1
```

Use an unused tag matching Cargo.toml; do not move or recreate an existing tag.
The **Native prerelease** workflow builds Windows x64, Linux x86-64 and macOS
Apple Silicon, tests the packages, then creates a **draft prerelease** with all
three platforms' assets and their smoke reports. Its combined `SHA256SUMS.txt` covers all binary archives,
all smoke reports and the matching source archive. A manual workflow run builds
artifacts for review without creating a GitHub release. A rerun may update an
existing draft; it refuses to replace assets in a published release.

In GitHub Releases, download and try the Windows ZIP, review all platforms'
smoke reports and notes,
then publish the draft. Builds do not publish the announcement or automatically
publish the release. The announcement draft is in docs/announcement-draft.md.

In Actions, manually run **Validate Linux and macOS native builds** to check
host compilation, interpreter, REPL, native -O0/-O2 and Rust unit tests on those
platforms. This records runner architecture and does not publish downloads.
Linux and Apple Silicon release checks also run automatically in **Native
prerelease**. Windows-only local packaging tests do not replace native Linux
or macOS execution checks.

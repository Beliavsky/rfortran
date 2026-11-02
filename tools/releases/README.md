# Native prerelease packaging

The prerelease workflow packages **Windows x64 and Linux x86-64**. `rfort.exe`
(Windows) or `rfort` (Linux) contains the compiler, interpreter and REPL; there
are no separate interpreter downloads. Linux assets are eligible for the draft
release only after the extracted archive passes on Ubuntu 22.04 and 24.04.
macOS remains a validation-only target with no release download.

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
each platform's Cargo target; Linux does not inherit the Windows dependency list.

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

## Create a draft prerelease

Commit all release changes, including Cargo.toml, Cargo.lock, release scripts,
examples, workflows and documentation. The tag version must match Cargo.toml.

```cmd
git push origin main
git tag -a v0.1.0-alpha.1 -m "Experimental Windows and Linux prerelease"
git push origin v0.1.0-alpha.1
```

Use an unused tag matching Cargo.toml; do not move or recreate an existing tag.
The **Native prerelease** workflow builds Windows x64 and Linux x86-64, tests the
packages, then creates a **draft prerelease** with both platforms' assets and
their smoke reports. Its combined `SHA256SUMS.txt` covers both binary archives,
all smoke reports and the matching source archive. A manual workflow run builds
artifacts for review without creating a GitHub release. A rerun may update an
existing draft; it refuses to replace assets in a published release.

In GitHub Releases, download and try the Windows ZIP, review both platforms'
smoke reports and notes,
then publish the draft. Builds do not publish the announcement or automatically
publish the release. The announcement draft is in docs/announcement-draft.md.

In Actions, manually run **Validate Linux and macOS native builds** to check
host compilation, interpreter, REPL, native -O0/-O2 and Rust unit tests on those
platforms. This records runner architecture and does not publish downloads.
Linux release checks also run automatically in **Native prerelease**. macOS
downloads should be added only after its checks and installation instructions
have been validated.

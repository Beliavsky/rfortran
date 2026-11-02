# Native prerelease packaging

The first binary release is **Windows x64, v0.1.0-alpha.1**. `rfort.exe` contains
the compiler, interpreter and REPL; there are no separate interpreter downloads.
Linux and macOS release downloads will follow successful platform validation.

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
Version, help, arrays, modules/functions and standard input are exercised.

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
the origin is recorded in THIRD-PARTY-LICENSES/index.json.

## Create the first draft prerelease

Commit all release changes, including Cargo.toml, Cargo.lock, release scripts,
examples, workflows and documentation. The tag version must match Cargo.toml.

```cmd
git push origin main
git tag -a v0.1.0-alpha.1 -m "First experimental Windows prerelease"
git push origin v0.1.0-alpha.1
```

The **Native prerelease** workflow builds and tests Windows x64, then creates a
**draft prerelease** with the verified assets. A manual workflow run builds an
artifact for review without creating a GitHub release. A rerun may update an
existing draft; it refuses to replace assets in a published release.

In GitHub Releases, download and try the ZIP, review the smoke report and notes,
then publish the draft. Builds do not publish the announcement or automatically
publish the release. The announcement draft is in docs/announcement-draft.md.

In Actions, manually run **Validate Linux and macOS native builds** to check
host compilation, interpreter, REPL, native -O0/-O2 and Rust unit tests on those
platforms. This records runner architecture and does not publish downloads.
Only add a platform to the release workflow after its checks and installation
instructions have been validated.

# rfortran browser interpreter

This playground runs the existing Rust interpreter as WebAssembly, entirely in
an ordinary browser. Its interface adapts the MIT-licensed
[ofort playground](https://beliavsky.github.io/ofort/). Program and REPL modes,
standard input, examples, source downloads, variable inspection and editable bug
reports work without a native execution server. CodeMirror and the WASI host are
vendored; execution needs no CDN.

The editor always uses three-space automatic Fortran block indentation, with
spaces rather than tabs. Program, module and procedure bodies start in column
one; each nested control block adds three spaces. Enter indents the next line; closing statements dedent
automatically. Tab and Shift+Tab indent or unindent selections. Shift+Enter in
the REPL adds an indented line; pasted source is indented in the main editor.
Enter still submits the REPL command. There is no
indentation setting to configure.

## Build and run locally

From the repository root, with Rust, Python and Node.js 22 or later installed:

```cmd
rustup target add wasm32-wasip1
python web-interpreter/build.py
node --test web-interpreter/tests/*.test.mjs
python -m http.server 8000 --bind 127.0.0.1 --directory web-interpreter
```

Open http://127.0.0.1:8000/. Use HTTP rather than opening index.html directly:
module workers need an HTTP origin. The first run downloads and compiles the
roughly 10 MB release interpreter. Subsequent requests reuse the compiled module.
`build.py --debug` builds faster but creates a much larger download.
Generated `runtime/rfort-browser.wasm`, `runtime/build.json` and copied license files are ignored by Git.
Rebuild after changing Rust sources. The native compiler executable is not used.

In Program mode, enter a complete free-form program. Same-source modules and
procedures use the existing compiler frontend. Supply READ input before Run.
In REPL mode, try:

```fortran
integer :: n
n = 7
n*n
real :: x(3)
x = [1.0, 2.0, 3.0]
sum(x)
.vars
```

The REPL shares the terminal REPL's scalar and INTEGER/REAL/LOGICAL array subset,
expressions, declaration inference, LET/CONST shortcuts, continuations, and
multiline blocks. Enter explicit END statements to finish blocks. `.help`,
`.vars`, `.info`, `.shapes`, `.sizes`, `.list`, `.auto on/off`, `.cancel` and
`.clear` inspect or manage the session. Auto-declare supports suitable rank-one
array assignments as well as scalars. Auto-end is unavailable.
READ, file I/O, modules and procedures require Program mode.

Download source exports a completed session as a compilable Fortran program.
Editing the source does not mutate current values: Restart and run source starts
a new session from it. Earlier statements are not replayed on each submission.
A failed submission retains previously committed values; STOP also aborts that
transaction. Commands accepted earlier in a multiline submission remain committed
when a later command returns a normal diagnostic. A process exit discards that
entire transaction. Stop, timeout, Clear session, switching modes and reloading
discard the session. Cancel a pending block with `.cancel`.

## Architecture and limits

`wasm/Cargo.toml` is an independent Cargo workspace without Cranelift or the
native linker. `src/browser_main.rs` uses the same module declarations, parser,
semantic passes, lowering and interpreter as `src/main.rs`. Source preparation
is shared in `src/frontend.rs`. `src/repl_browser.rs` serializes the native REPL
Session; JavaScript stores snapshots but does not parse Fortran or evaluate it.

`runtime-adapter.mjs` supplies WASI stdin, stdout/stderr and an in-memory
filesystem using the MIT-licensed browser_wasi_shim. Each operation starts a
fresh WASI instance, allowing real Rust STOP/error exits without closing the
page. A successful REPL response commits its snapshot. Program virtual files
are discarded after execution; they cannot access the user's disk. File upload
and file download are not exposed. Rust memory is capped at 256 MiB, with an
8 MiB stack; each output channel is limited to 1 MB, cumulative virtual file
writes to 2 MB, and descriptors per operation to 128. Source/input requests are limited
to 200,000 characters each. A worker is terminated after 30 seconds or Stop.

Browser support is the interpreter's supported subset, not the native backend's
full feature set. There is no native code generation, OS process launching,
external libraries, or C interoperability in this build. Large REPL snapshots
can exceed the virtual file limit and leave the previous state intact.

Program drafts and supplied input are stored locally when browser storage is
available. REPL state is not saved across reloads. Source is never sent to an
execution service. Report a bug creates an editable preview; opening a GitHub
issue is explicit, source is excluded by default, and input is not included
automatically. See THIRD-PARTY-NOTICES.txt for licenses. The combined interpreter
is GPLv3; builds include its license and a source revision link. Vendored UI
and WASI host code retain their MIT notices.

## Tests

```cmd
set RFORT_WASM_REQUIRED=1
python web-interpreter/tests/test_package.py
node --test web-interpreter/tests/*.test.mjs
```

Without a built artifact, WASM tests skip unless RFORT_WASM_REQUIRED is set.
RFORT_WASM_PATH can select a development binary. Integration tests execute the
actual binary using the same WASI shim as the worker: programs, input, errors,
STOP, arrays, functions, REPL persistence, blocks, inference and export/replay.
Driver and UI tests use Worker/DOM stubs for stop/timeout, edits, reports and
controls; they do not replace a rendered browser check.

## GitHub Pages

`python web-interpreter/package.py` prepares `target/browser-site` for static
hosting. Its HTML uses content hashes in script and stylesheet URLs so browser
caches cannot combine a new page with old editor assets. To try the packaged
site locally, serve that directory instead of `web-interpreter`. Packaging tests
verify that an editor change also changes its published URL.

`.github/workflows/browser-interpreter.yml` builds and tests on every push
and pull requests, and uploads a ready-to-serve static site artifact. It builds
on a Windows runner, matching the locally validated interpreter build; the
resulting WebAssembly runs in browsers on Windows, Linux and macOS. No server
or secrets are needed. To publish it:

1. In repository Settings / Pages, select GitHub Actions as the build source.
2. Commit and push the workflow and interpreter sources to the default branch
   (`main`), or run the Browser interpreter workflow manually after pushing.
3. In Actions, wait for the Browser interpreter build, tests and deployment to
   succeed. The published URL appears in the deployment job.

The site will be served at https://beliavsky.github.io/rfortran/ and update with
every successful push to the default branch. Failed builds/tests keep the last
published version. Pull requests and other branches build and test without
publishing. No API service URL, server, token or secret is needed.

Publishing is enabled by default. To disable browser deployment, set repository
Actions variable `BROWSER_INTERPRETER_PAGES` to `false`. The older server-based
playground can publish only when that variable is `false` and
`PLAYGROUND_API_URL` is set, so the two workflows cannot overwrite one another.
Deploying replaces that repository's Pages site. The local server is only for
development; public visitors do not need Python, Rust or rfort installed.

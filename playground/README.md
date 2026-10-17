# rfortran playground

A browser frontend for **rfort only**, with multiple source files, standard input,
compiler diagnostics, optimization selection, and local draft storage. It uses the
existing native compiler. No compiler, interpreter, or transpiler changes are needed.

## Try it on Windows

From the repository root, using your existing compiler:

```cmd
python playground-service\server.py --local-compiler target\release\rfort.exe
```

Open <http://127.0.0.1:8080>. Stop the server with Ctrl+C. The server snapshots the
executable at startup, so restart it after updating rfort. Local mode runs trusted
programs with your account's privileges; it is deliberately restricted to loopback
and refuses additional browser origins. Use container mode for a public service.

Sources are sent only when Compile or Compile & run is pressed. Drafts and the
optional service URL are saved in browser localStorage. Files compile left to
right; put dependency modules first and the main program last. Compiled executables
are discarded after each job. Compile & run always rebuilds the program.

## Publish from this repository

The new `rfortran-playground.yml` workflow tests the frontend and service, builds
and smoke-tests a Linux sandbox image, and publishes that image to GHCR after a
push to the default branch. A failed build or sandbox smoke test prevents publishing.

To also deploy the website:

1. Provision the execution host described in
   [the service guide](../playground-service/README.md).
2. Set repository **Settings → Secrets and variables → Actions → Variables →
   `PLAYGROUND_API_URL`** to its HTTPS origin, e.g. `https://rfort.example.org`.
3. Set **Settings → Pages → Build and deployment → Source → GitHub Actions**.
4. Push to the default branch or run the workflow manually.

The Pages site will be `https://beliavsky.github.io/rfortran/`. Without
`PLAYGROUND_API_URL`, CI still validates and publishes the image but deliberately
skips Pages deployment. GitHub Pages serves the editor; it cannot execute native
Fortran programs. A browser-only WebAssembly compiler is not part of this change.

For automatic execution-host updates, configure the optional SSH deployment
variables and secrets in the service guide. Publishing a new image alone does not
restart a running service. The deployment workflow never modifies compiler sources.

## Frontend tests

```cmd
node --test playground/tests/model.test.mjs
```

The frontend is buildless and has no external JavaScript, fonts, or CDN dependencies.
`config.json` contains `api_base`; an empty value uses the current origin for local
testing. The Pages workflow sets the deployed copy without editing the source file.
“Service connection” lets you override it in your own browser. The rfort API is
separate from the session/job API used by the existing Fortran playground.

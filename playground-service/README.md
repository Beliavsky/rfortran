# rfort execution service

This Python 3.11+ gateway and worker use only the standard library. They invoke the
existing native rfort compiler with fixed options; they do not invoke the interpreter
or transpiler. Run from the repository root unless otherwise specified.

## Local use and tests

```cmd
python playground-service\server.py --local-compiler target\release\rfort.exe
```

Open <http://127.0.0.1:8080>. Local mode is for **your own trusted code**. It is not a
sandbox and must not be exposed through a proxy, tunnel, or port forward. It binds
to loopback, checks the Host header, and disallows cross-origin browser access.

```cmd
set RFORT_PLAYGROUND_COMPILER=%CD%\target\release\rfort.exe
python -m unittest discover -s playground-service/tests -v
node --test playground/tests/model.test.mjs
```

Without `RFORT_PLAYGROUND_COMPILER`, native integration tests are skipped; protocol,
HTTP, timeout, and output-limit tests still run. No Rust builds are performed by
these tests. The Docker smoke test additionally checks native Linux compilation,
input, modules, and optimization inside the production sandbox.

## Public execution host

Use a dedicated Linux worker host with Docker Engine, Python 3.11+, and an HTTPS
reverse proxy. Treat submitted programs as hostile. Every job runs in a fresh,
unprivileged, read-only container with no network, capabilities, host mounts, or
Docker socket; limits include 256 MiB memory, one CPU, 64 processes, bounded tmpfs,
20-second compilation, 3-second execution, and bounded output. Docker isolation
still shares the host kernel; keep the host patched and separate from sensitive
services. CORS is a browser policy, not authentication. The anonymous API has two
execution slots and per-client throttling; use reverse-proxy/global rate limits
for Internet traffic. Authentication or stronger VM isolation can be added later.

Build and validate an image on a Linux Docker host:

```sh
docker build -f playground-service/Dockerfile -t rfort-playground .
python3 playground-service/smoke.py rfort-playground
python3 playground-service/server.py --image rfort-playground --origin https://beliavsky.github.io
```

The gateway remains on `127.0.0.1:8080`, behind your HTTPS reverse proxy. It fails
closed if Docker or the image is unavailable. The HTTP gateway account needs Docker
access; Docker group membership is effectively host administrator access. Never
place the Docker socket in a job container. The compiler image contains the Linux
compiler and C linker/runtime, not the HTTP gateway. Builds use the repository's
tracked Cargo.lock with `--locked`; deployed images are selected by immutable commit
tags. Pin Rust and Debian base-image digests for stronger build reproducibility.

Example Caddy configuration (replace the hostname):

```caddyfile
rfort.example.org {
    request_body {
        max_size 256KB
    }
    reverse_proxy 127.0.0.1:8080
}
```

Forward only from the proxy to the gateway. The gateway does not trust client-supplied
`X-Forwarded-For`; behind this proxy its throttle is shared across users. Configure
the proxy's own global/IP limits for expected traffic. Do not cache API responses.
Request bodies/source are not logged by this service; do not enable body logging
at the proxy. Use a single gateway process so its concurrency limit is global.

## Automatic updates

The workflow publishes `ghcr.io/beliavsky/rfortran-playground:<commit>` and `:latest`
after checks pass. Make the GHCR package public so a worker can pull it without a
stored GitHub token. For another repository, the image prefix uses that owner/repo.

Provision `/opt/rfort-playground` on the host with copies of `server.py`, `worker.py`,
`protocol.py`, `deploy.sh`, and `image.env`, owned by a dedicated account. The gateway
files and systemd template live in this new directory; keep their host copies up to
date when the API changes. The SSH deployment updates both gateway files and image.
Install `rfort-playground.service` into `/etc/systemd/system/`, adjust the account,
path, and allowed origin if needed, then `systemctl daemon-reload`.

Create `/opt/rfort-playground/image.env` initially:

```sh
RFORT_PLAYGROUND_IMAGE=ghcr.io/beliavsky/rfortran-playground:COMMIT
```

Pull that image and start `rfort-playground.service`. Point the HTTPS proxy at it.
The deployment account must own `/opt/rfort-playground`, have Docker access, and
have passwordless sudo restricted to `systemctl restart rfort-playground.service`.
Create it under the name `rfort-playground`, matching the service template, or edit
the template. SSH is optional: you can deploy image updates manually instead.

Configure these GitHub Actions settings:

| Kind | Name | Value |
| --- | --- | --- |
| Variable | `PLAYGROUND_API_URL` | Public HTTPS service origin |
| Variable | `PLAYGROUND_SSH_HOST` | Dedicated worker hostname |
| Variable | `PLAYGROUND_SSH_USER` | Deployment account |
| Secret | `PLAYGROUND_SSH_KEY` | SSH private key for that account |
| Secret | `PLAYGROUND_SSH_KNOWN_HOSTS` | Independently verified SSH host-key entry |

Without the SSH host variable, image publication and configured Pages deployment
still work, but the execution host must be updated manually. With it, each default
branch push deploys the tested commit image, restarts the service, and checks health.
If the health check fails, the deployment script restores the previous image setting.
The SSH key cannot be used by pull-request jobs. No action here pushes repository
commits or changes compiler sources.

## API

`GET /api/health` returns `compiler: "rfort"`, execution mode, limits, and the
container image's compiler commit (or the executable's version in local mode).
`POST /api/compile` accepts JSON:

```json
{
  "action": "run",
  "files": [{"name": "main.f90", "content": "print *, 42\nend\n"}],
  "stdin": "",
  "optimization": "0",
  "warnings": false
}
```

Use `action: "compile"` to skip execution. Sources are ordered, with dependencies
before their users. Limits: eight files, 128 KiB total source, 32 KiB standard input,
256 KiB HTTP request, 64 KiB stage output, and a 30-second outer worker timeout.
The outer response cap may also truncate a job with heavily escaped output. Inputs
cannot select another compiler, arbitrary flags, command lines, environment variables,
or filesystem paths. Temporary files are discarded after each job. Responses contain
`build` and optional `run`, each with stdout, stderr, exit code, elapsed seconds,
timeout and output-limit flags. Compilation failures are successful HTTP responses
with a nonzero build exit code; malformed requests return 4xx, capacity limits 429,
and worker infrastructure failures 503.

#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Run a Fortran program with the rfortran playground's interpreter, without
installing rfortran.

The playground at https://beliavsky.github.io/rfortran/ runs the rfortran
interpreter compiled to WebAssembly. This script downloads that same module
(once; it is cached), runs it on your machine with the wasmtime package, and
prints the program's output, so results match the playground. The program
runs sandboxed in a temporary folder: it sees only its source, its standard
input and the files it creates, which are deleted afterwards.

    pip install wasmtime
    python rfort_web_run.py hello.f90
    python rfort_web_run.py -Wall -Wstyle --check hello.f90
    python rfort_web_run.py mod.f90 main.f90 --stdin input.txt

Several sources are joined in order into one program, as in the playground's
editor: put modules first. Program output goes to standard output, and
warnings and errors to standard error. The exit status is the program's.
"""
import argparse
import hashlib
import json
import os
import shutil
import sys
import tempfile
import threading
import urllib.request
from pathlib import Path

SITE = "https://beliavsky.github.io/rfortran/"
WARNINGS = ("-Wall", "-Wextra", "-Wstyle", "-Weverything")
# What the interpreter accepts beyond running a program, as build.json lists it.
NEEDS = {"check": "--check", "warnings": "-Wall, -Wextra and -Wstyle", "everything": "-Weverything"}


def cache_dir():
    base = os.environ.get("LOCALAPPDATA") or os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache"
    path = Path(base) / "rfortran-web"
    path.mkdir(parents=True, exist_ok=True)
    return path


def fetch(url, limit=64 << 20):
    request = urllib.request.Request(url, headers={"User-Agent": "rfort-web-run"})
    with urllib.request.urlopen(request, timeout=60) as response:
        data = response.read(limit + 1)
    if len(data) > limit:
        raise RuntimeError(f"{url} is larger than expected")
    return data


def site_module(site, refresh):
    """The site's interpreter module and its build information, downloaded
    once and kept while the site's build stays the same."""
    info = json.loads(fetch(site + "runtime/build.json"))
    path = cache_dir() / f"rfort-browser-{info['sha256'][:16]}.wasm"
    if refresh or not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != info["sha256"]:
        print(f"Downloading the rfortran interpreter ({info.get('bytes', 0) / 1e6:.0f} MB, build {info.get('commit')})...",
              file=sys.stderr)
        data = fetch(f"{site}runtime/rfort-browser.wasm?v={info['sha256']}")
        if hashlib.sha256(data).hexdigest() != info["sha256"]:
            raise RuntimeError("the downloaded interpreter does not match the site's build.json")
        path.write_bytes(data)
    return path, info


def local_module(path):
    """A locally built module, with the build.json beside it if there is one."""
    path = Path(path)
    info_path = path.with_name("build.json")
    info = json.loads(info_path.read_text(encoding="utf-8")) if info_path.is_file() else {}
    info.setdefault("features", list(NEEDS))
    return path, info


def compiled(wasmtime, engine, path):
    """The module, compiled once per module and wasmtime version: compiling
    takes several seconds, loading the result a fraction of one."""
    digest = hashlib.sha256(path.read_bytes()).hexdigest()[:16]
    version = getattr(wasmtime, "__version__", None) or _wasmtime_version()
    native = cache_dir() / f"rfort-browser-{digest}-wasmtime-{version}.cwasm"
    if native.is_file():
        try:
            return wasmtime.Module.deserialize_file(engine, str(native))
        except Exception:
            native.unlink(missing_ok=True)
    print("Compiling the interpreter for this machine (once)...", file=sys.stderr)
    module = wasmtime.Module.from_file(engine, str(path))
    try:
        native.write_bytes(module.serialize())
    except OSError:
        pass
    return module


def _wasmtime_version():
    try:
        from importlib.metadata import version
        return version("wasmtime")
    except Exception:
        return "unknown"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0],
                                     epilog="Options are those of the playground; see " + SITE)
    parser.add_argument("sources", nargs="+", type=Path, help="Fortran source files, joined in order")
    for option, text in zip(WARNINGS, ("common warnings", "opt-in advice that -Wall leaves out", "style advice",
                                       "every warning")):
        parser.add_argument(option, action="append_const", const=option, dest="warnings", help=f"enable {text}")
    parser.add_argument("--check", action="store_true", help="report errors and warnings without running the program")
    parser.add_argument("--stdin", metavar="FILE", help="standard input for the program; - reads this script's input")
    parser.add_argument("--timeout", type=float, default=30, metavar="SECONDS",
                        help="stop the program after this long (default 30; 0 for no limit)")
    parser.add_argument("--wasm", metavar="FILE", help="use this rfort-browser.wasm instead of the site's")
    parser.add_argument("--site", default=SITE, help="playground URL (default %(default)s)")
    parser.add_argument("--refresh", action="store_true", help="download the interpreter again")
    args = parser.parse_args(argv)
    warnings = list(dict.fromkeys(args.warnings or []))

    try:
        import wasmtime
    except ImportError:
        print("This script needs the wasmtime package: pip install wasmtime", file=sys.stderr)
        return 2
    try:
        text = "\n".join(p.read_text(encoding="utf-8") for p in args.sources)
        stdin = b"" if args.stdin is None else sys.stdin.buffer.read() if args.stdin == "-" else Path(args.stdin).read_bytes()
        site = args.site if args.site.endswith("/") else args.site + "/"
        path, info = local_module(args.wasm) if args.wasm else site_module(site, args.refresh)
    except (OSError, ValueError, RuntimeError) as error:
        print(f"rfort_web_run: {error}", file=sys.stderr)
        return 2
    features = set(info.get("features", []))
    for feature, wanted in (("check", args.check), ("warnings", bool(warnings)),
                            ("everything", "-Weverything" in warnings)):
        if wanted and feature not in features:
            print(f"rfort_web_run: the playground's interpreter (build {info.get('commit')}) does not support "
                  f"{NEEDS[feature]} yet", file=sys.stderr)
            return 2

    config = wasmtime.Config()
    config.epoch_interruption = True
    engine = wasmtime.Engine(config)
    module = compiled(wasmtime, engine, path)
    work = Path(tempfile.mkdtemp(prefix="rfort-web-"))
    try:
        (work / "program.f90").write_text(text, encoding="utf-8")
        (work / ".stdin").write_bytes(stdin)
        wasi = wasmtime.WasiConfig()
        wasi.argv = ["rfort-browser", *(["check"] if args.check else []), *warnings]
        wasi.env = [("NO_COLOR", "1"), ("TERM", "dumb")]
        wasi.preopen_dir(str(work), "/")
        wasi.stdin_file = str(work / ".stdin")
        wasi.stdout_file = str(work / ".stdout")
        wasi.stderr_file = str(work / ".stderr")
        store = wasmtime.Store(engine)
        store.set_wasi(wasi)
        store.set_epoch_deadline(1)
        linker = wasmtime.Linker(engine)
        linker.define_wasi()
        timer = threading.Timer(args.timeout, engine.increment_epoch) if args.timeout > 0 else None
        if timer:
            timer.daemon = True
            timer.start()
        status, failure = 0, ""
        try:
            linker.instantiate(store, module).exports(store)["_start"](store)
        except wasmtime.ExitTrap as exit_trap:
            status = exit_trap.code
        except wasmtime.Trap as trap:
            status = 1
            timed_out = timer is not None and not timer.is_alive()
            failure = (f"Stopped after {args.timeout:g} seconds." if timed_out
                       else f"WebAssembly runtime error: {trap.message}")
        finally:
            if timer:
                timer.cancel()
        sys.stdout.write((work / ".stdout").read_text(encoding="utf-8", errors="replace"))
        sys.stdout.flush()
        # Diagnostics name the sandbox's /program.f90; name the user's source.
        name = args.sources[0].name if len(args.sources) == 1 else "program.f90"
        errors = (work / ".stderr").read_text(encoding="utf-8", errors="replace").replace("/program.f90", name)
        errors += failure + "\n" if failure else ""
        try:
            errors.encode(sys.stderr.encoding or "ascii")
        except (UnicodeEncodeError, LookupError):
            # A console without UTF-8 gets the diagnostics' frames in ASCII.
            errors = errors.translate(str.maketrans("┌─│", "+-|"))
        sys.stderr.write(errors)
        return status
    finally:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())

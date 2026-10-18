"""Try the partial Rust transpiler on files/globs and save a JSON result report."""
from __future__ import annotations

import argparse
from collections import Counter
import glob
import hashlib
import json
import math
from pathlib import Path
import subprocess
import sys
import time


TRANSPILER = Path(__file__).resolve().with_name("transpile.py")


def collect_inputs(patterns):
    """Preserve pattern order, sort matches, and process each actual file once."""
    files, errors, seen = [], [], set()
    for pattern in patterns:
        literal = Path(pattern)
        matches = [literal] if literal.exists() else [Path(p) for p in sorted(glob.glob(pattern, recursive=True))]
        if not matches:
            errors.append({"source": pattern, "status": "input_failed", "diagnostic": "No matching files", "stages": []})
        for path in matches:
            if not path.is_file():
                errors.append({"source": str(path), "status": "input_failed", "diagnostic": "Not a file", "stages": []})
                continue
            path = path.resolve()
            if path not in seen:
                seen.add(path)
                files.append(path)
    return files, errors


def artifact_dir(root, source, modules=()):
    # Keep same-named files from different directories distinct, independent of
    # selection order. Only a safe basename and a path digest become output paths.
    paths = [*modules,source]
    digest = hashlib.sha256("\0".join(str(path.resolve()) for path in paths).encode("utf-8")).hexdigest()[:16]
    return root / f"{source.stem}-{digest}"


def run_phase(name, command, cwd, timeout):
    started = time.perf_counter()
    result = {"stage": name, "command": [str(arg) for arg in command], "cwd": str(cwd)}
    try:
        process = subprocess.run(result["command"], cwd=cwd, stdin=subprocess.DEVNULL,
                                 capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=timeout)
        result.update(status="ok" if process.returncode == 0 else "failed",
                      returncode=process.returncode, stdout=process.stdout, stderr=process.stderr)
    except subprocess.TimeoutExpired as error:
        def decoded(value):
            return value.decode("utf-8", errors="replace") if isinstance(value, bytes) else value or ""
        result.update(status="timeout", returncode=None, stdout=decoded(error.stdout),
                      stderr=decoded(error.stderr), diagnostic=f"Timed out after {timeout:g} seconds")
    except OSError as error:
        result.update(status="failed", returncode=None, stdout="", stderr="", diagnostic=str(error))
    result["seconds"] = round(time.perf_counter() - started, 3)
    return result


def try_file(source, args):
    work = artifact_dir(args.out_dir, source, args.modules)
    result = {"source": str(source), "artifact_dir": str(work), "stages": []}
    try:
        work.mkdir(parents=True, exist_ok=True)
    except OSError as error:
        return dict(result, status="output_failed", diagnostic=str(error))
    rust = work / "main.rs"
    executable = work / ("main.exe" if sys.platform == "win32" else "main.bin")
    command = [sys.executable, TRANSPILER, *args.modules, source, "-o", rust]
    if args.rfort:
        command += ["--rfort", args.rfort]
    phases = [("translate", command, args.translate_timeout)]
    if args.run:
        phases += [
            ("compile", [args.rustc, "--edition=2021", "--crate-name", "rfort_batch", "-D", "warnings", rust, "-o", executable], args.compile_timeout),
            ("run", [executable], args.run_timeout),
        ]
    for name, command, timeout in phases:
        phase = run_phase(name, command, work, timeout)
        result["stages"].append(phase)
        if phase["status"] != "ok":
            result["status"] = name + "_" + phase["status"]
            result["diagnostic"] = phase.get("diagnostic") or phase["stderr"].strip() or phase["stdout"].strip() or f"Exit status {phase['returncode']}"
            return result
    result["status"] = "passed" if args.run else "translated"
    return result


def positive_seconds(value):
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError("timeout must be finite and positive")
    return number


def positive_integer(value):
    try:
        number = int(value)
    except ValueError:
        raise argparse.ArgumentTypeError("limit must be a positive integer") from None
    if number <= 0:
        raise argparse.ArgumentTypeError("limit must be a positive integer")
    return number


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inputs", nargs="+", help='Files or globs, e.g. "*.f90" or "tests/**/*.f90"')
    parser.add_argument("--run", action="store_true", help="Also compile and execute each successful translation")
    parser.add_argument("--limit", type=positive_integer, help="Try at most N driver files after deduplication and module exclusion", metavar="N")
    parser.add_argument("--module", dest="modules", action="extend", nargs="+", type=Path, default=[],
                        help="Prepend one or more common module sources in dependency order; may be repeated")
    parser.add_argument("--out-dir", type=Path, default=Path("target/rust-transpiler/batch"))
    parser.add_argument("--report", type=Path, help="JSON report path (default: OUT_DIR/report.json)")
    parser.add_argument("--rfort", type=Path, help="Existing compiler executable; never rebuilt")
    parser.add_argument("--rustc", default="rustc", help="rustc executable name or path")
    parser.add_argument("--translate-timeout", type=positive_seconds, default=120)
    parser.add_argument("--compile-timeout", type=positive_seconds, default=120)
    parser.add_argument("--run-timeout", type=positive_seconds, default=10)
    parser.add_argument("--verbose", action="store_true", help="Print complete diagnostics and captured program output")
    args = parser.parse_args(argv)
    args.out_dir = args.out_dir.resolve()
    args.report = args.report.resolve() if args.report else args.out_dir / "report.json"
    if args.rfort:
        args.rfort = args.rfort.resolve()
    if Path(args.rustc).exists():
        args.rustc = str(Path(args.rustc).resolve())
    sources, results = collect_inputs(args.inputs)
    args.modules = [path.resolve() for path in args.modules]
    sources = [path for path in sources if path not in args.modules]
    if not sources and not results:
        parser.error("no driver sources selected after excluding --module inputs")
    if args.report in [*sources,*args.modules]:
        parser.error("report path must differ from an input source")
    selected = sources[:args.limit] if args.limit is not None else sources
    skipped = sources[len(selected):]
    started = time.perf_counter()
    for source in selected:
        result = try_file(source, args)
        results.append(result)
        print(f"{result['status'].upper():18} {source}", flush=True)
        if result.get("diagnostic"):
            diagnostic = result["diagnostic"]
            print(diagnostic if args.verbose else "  " + diagnostic.splitlines()[0], flush=True)
        if args.verbose:
            for stage in result["stages"]:
                for stream in ("stdout", "stderr"):
                    if stage.get(stream):
                        print(f"[{stage['stage']} {stream}]\n{stage[stream]}", end="" if stage[stream].endswith("\n") else "\n")
    for result in results:
        if result["status"] == "input_failed":
            print(f"INPUT_FAILED       {result['source']}: {result['diagnostic']}")
    counts = dict(sorted(Counter(result["status"] for result in results).items()))
    report = {"schema_version": 1, "mode": "run" if args.run else "translate", "patterns": args.inputs,
              "modules": [str(path) for path in args.modules],
              "limit": args.limit, "drivers_matched": len(sources), "drivers_selected": len(selected),
              "drivers_skipped": [str(path) for path in skipped],
              "seconds": round(time.perf_counter()-started, 3), "summary": counts, "results": results}
    try:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2, ensure_ascii=False)+"\n", encoding="utf-8")
    except OSError as error:
        print(f"Cannot write report: {error}", file=sys.stderr)
        return 1
    print("\nSummary: " + ", ".join(f"{count} {status}" for status, count in counts.items()))
    if args.limit is not None:
        print(f"Drivers: {len(selected)} selected of {len(sources)}; {len(skipped)} skipped by --limit")
    print(f"Report: {args.report}")
    return int(any(result["status"] not in ("translated", "passed") for result in results))


if __name__ == "__main__":
    sys.exit(main())

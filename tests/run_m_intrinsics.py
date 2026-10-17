"""Inventory M_intrinsics demos against an existing native rfort executable.

No compiler build, interpreter invocation, or upstream source modification.
Compilation is the default; --run compares eligible demos with gfortran.
"""
import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import tempfile
import time

from run_ofort_compat import equivalent, source_code

ROOT = Path(__file__).resolve().parents[1]
EXE = "program.exe" if os.name == "nt" else "program"


def invoke(command, cwd, timeout, stdin=""):
    command = list(map(str, command))
    result = {"command": command}
    start = time.monotonic()
    try:
        options = ({"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}
                   if os.name == "nt" else {"start_new_session": True})
        process = subprocess.Popen(command, cwd=cwd, stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   text=True, encoding="utf-8", errors="replace", **options)
        try:
            out, err = process.communicate(stdin, timeout=timeout)
        except subprocess.TimeoutExpired:
            result["timeout"] = True
            if os.name == "nt":
                subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                               capture_output=True, timeout=10)
            else:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            if process.poll() is None:
                process.kill()
            out, err = process.communicate()
        result.update(returncode=process.returncode, stdout=out, stderr=err)
    except OSError as error:
        result.update(launch_error=True, stderr=str(error))
    result["seconds"] = round(time.monotonic() - start, 4)
    return result


def failed(result, stage):
    if result.get("timeout"):
        return stage + "_timeout"
    if result.get("launch_error"):
        return stage + "_launch_error"
    code = result["returncode"]
    if code:
        crash = code < 0 or code == 101 or code >= 0x80000000
        return stage + ("_crash" if crash else "_error")
    return None


def runtime_exclusion(text):
    """Conservative triage, not an execution sandbox or a validity judgment."""
    code = source_code(text)
    checks = [
        (r"\b(?:execute_command_line|system|sleep|abort|exit)\s*\(",
         "external commands or process control"),
        (r"\b(?:random_number|random_seed|cpu_time|system_clock|date_and_time)\s*\(",
         "randomness or timing needs a tailored oracle"),
        (r"\b(?:get_command|get_command_argument|get_environment_variable|"
         r"compiler_version|compiler_options|command_argument_count|getcwd)\s*\(",
         "arguments, environment, or compiler-dependent output"),
        (r"\b(?:read|open|close|inquire|rewind|backspace|endfile)\s*(?:\(|\*)",
         "I/O needs explicit input or file fixtures"),
        (r"\b(?:iso_c_binding|ieee_arithmetic|ieee_exceptions|ieee_features)\b|"
         r"\bbind\s*\(|\b(?:this_image|num_images|co_sum|co_max|co_min|co_broadcast|"
         r"co_reduce|image_status|failed_images|stopped_images)\s*\(",
         "interop, IEEE, or coarray demo needs a tailored oracle"),
        (r"(?m)^\s*(?:#\s*include|include\b)|\bexternal\b|\binterface\b",
         "external dependencies or interfaces need review"),
    ]
    for pattern, reason in checks:
        if re.search(pattern, code):
            return reason
    return None


def discover(root, patterns, exclusions):
    sources = sorted(p for p in (root / "example").iterdir()
                     if p.is_file() and p.suffix.lower() in {".f90", ".f95", ".f03", ".f08"})
    return [p for p in sources
            if any(fnmatch.fnmatchcase(p.name.lower(), pat.lower()) for pat in patterns)
            and not any(fnmatch.fnmatchcase(p.name.lower(), pat.lower()) for pat in exclusions)]


def probe(source, compiler, reference, levels, timeout, execute, reference_args):
    content = source.read_bytes()
    text = content.decode("utf-8", errors="replace")
    reason = runtime_exclusion(text)
    item = {"source": source.name, "sha256": hashlib.sha256(content).hexdigest(),
            "runtime_exclusion": reason, "results": {}}
    with tempfile.TemporaryDirectory(prefix="rfort-intrinsics-", ignore_cleanup_errors=True) as tmp:
        directory = Path(tmp)
        reference_output = None
        reference_status = None
        if reference:
            work = directory / "reference"
            work.mkdir()
            copied = work / source.name
            copied.write_bytes(content)
            executable = work / EXE
            build = invoke([reference, "-O0", *reference_args, copied, "-o", executable], work, timeout)
            ref = item["reference"] = {"build": build}
            reference_status = failed(build, "reference_compile")
            if not reference_status and execute and not reason:
                result = invoke([executable], work, timeout)
                ref["run"] = result
                reference_status = failed(result, "reference_run")
                if not reference_status:
                    # Repeat in a fresh directory to detect observable instability.
                    repeat = directory / "reference_repeat"
                    repeat.mkdir()
                    repeated = invoke([executable], repeat, timeout)
                    ref["repeat"] = repeated
                    reference_status = failed(repeated, "reference_run")
                    if not reference_status and (not equivalent(result["stdout"], repeated["stdout"])
                                                 or result["stderr"] != repeated["stderr"]):
                        reference_status = "reference_nondeterministic"
                    if not reference_status:
                        reference_output = result
            ref["status"] = reference_status or ("run_ok" if reference_output else "compiled")
        for level in levels:
            work = directory / ("O" + level)
            work.mkdir()
            copied = work / source.name
            copied.write_bytes(content)
            executable = work / EXE
            build = invoke([compiler, "-O", level, copied, "-o", executable], work, timeout)
            entry = item["results"]["O" + level] = {"build": build}
            status = failed(build, "compile")
            if not status:
                if not execute:
                    status = "compiled"
                elif reason:
                    status = "compiled_run_skipped"
                elif reference_status:
                    status = "compiled_no_reference_oracle"
                else:
                    actual = invoke([executable], work, timeout)
                    entry["run"] = actual
                    status = failed(actual, "run")
                    if not status:
                        if not reference_output:
                            status = "run_ok_unchecked"
                        elif (equivalent(actual["stdout"], reference_output["stdout"])
                              and actual["stderr"].strip() == reference_output["stderr"].strip()):
                            status = "pass"
                        else:
                            status = "wrong_output"
            entry["status"] = status
        statuses = [e["status"] for e in item["results"].values()]
        item["status"] = statuses[0] if len(set(statuses)) == 1 else "mixed"
    return item


def identity(path, timeout):
    return {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "version": invoke([path, "--version"], ROOT, timeout)}


def git_revision(directory):
    try:
        result = subprocess.run(["git", "-C", str(directory), "rev-parse", "HEAD"],
                                capture_output=True, text=True, timeout=10)
        return result.stdout.strip() if result.returncode == 0 else None
    except (OSError, subprocess.TimeoutExpired):
        return None


def summarize(report):
    report["summary"] = dict(Counter(c["status"] for c in report["cases"]))
    report["reference_summary"] = dict(Counter(c["reference"]["status"] for c in report["cases"]
                                              if "reference" in c))
    report["optimization_summary"] = {
        "O" + level: dict(Counter(c["results"]["O" + level]["status"] for c in report["cases"]))
        for level in report["optimization_levels"]}
    report["runtime_exclusions"] = dict(Counter(c["runtime_exclusion"] for c in report["cases"]
                                               if c["runtime_exclusion"]))


def markdown(report):
    lines = ["# M_intrinsics compatibility results", "", f"Generated: {report['created_utc']}",
             "", f"Upstream commit: `{report['upstream_commit']}`",
             f"Compiler SHA-256: `{report['compiler']['sha256']}`", "",
             "Compilation success is not an execution or conformance pass.",
             "Output comparisons ignore layout and allow numeric roundoff; exact integers are retained.",
             "Examples rejected by gfortran are not automatically invalid Fortran.", "",
             "## Summary", "", "```json", json.dumps(report["optimization_summary"], indent=2),
             "```", "", "## Cases", "", "| Example | rfort | gfortran | Runtime exclusion |",
             "|---|---|---|---|"]
    for item in report["cases"]:
        results = ", ".join(f"{opt}: {entry['status']}" for opt, entry in item["results"].items())
        lines.append(f"| {item['source']} | {results} | {item.get('reference', {}).get('status', 'not requested')} | {item['runtime_exclusion'] or ''} |")
    return "\n".join(lines) + "\n"


def executable(value):
    found = shutil.which(str(value))
    path = Path(found or value).resolve()
    if not path.is_file():
        raise ValueError(f"executable not found: {value}")
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--intrinsics-root", type=Path, required=True)
    parser.add_argument("--compiler", default=str(ROOT / "target/release" / ("rfort.exe" if os.name == "nt" else "rfort")))
    parser.add_argument("--reference", default="gfortran")
    parser.add_argument("--no-reference", action="store_true")
    parser.add_argument("--reference-arg", action="append", default=[])
    parser.add_argument("--filter", action="append", help="repeatable source filename glob")
    parser.add_argument("--exclude", action="append", default=[])
    parser.add_argument("--opt-level", action="append", choices=["0", "1", "2", "3", "s", "z"])
    parser.add_argument("--run", action="store_true", help="run eligible demos and compare against two reference runs")
    parser.add_argument("--jobs", type=int, default=1)
    parser.add_argument("--timeout", type=float, default=20)
    parser.add_argument("--limit", type=int)
    parser.add_argument("--report", type=Path, default=ROOT / "target/m-intrinsics.json")
    parser.add_argument("--strict", action="store_true", help="return nonzero on rfort failures or unchecked requested executions")
    args = parser.parse_args()
    if args.jobs < 1 or args.timeout <= 0 or (args.limit is not None and args.limit < 1):
        parser.error("jobs, timeout, and limit must be positive")
    root = args.intrinsics_root.resolve()
    if not (root / "example").is_dir():
        parser.error("intrinsics root must contain the upstream example directory")
    try:
        compiler = executable(args.compiler)
        reference = None if args.no_reference else executable(args.reference)
    except ValueError as error:
        parser.error(str(error))
    sources = discover(root, args.filter or ["demo_*"], args.exclude)
    if args.limit:
        sources = sources[:args.limit]
    if not sources:
        parser.error("no matching examples")
    levels = list(dict.fromkeys(args.opt_level or ["0", "2"]))
    report = {"schema_version": 1, "created_utc": datetime.now(timezone.utc).isoformat(),
              "upstream_root": str(root), "upstream_commit": git_revision(root),
              "mode": "native_compare" if args.run else "native_compile",
              "optimization_levels": levels, "filters": args.filter or ["demo_*"],
              "compiler": identity(compiler, args.timeout),
              "reference_compiler": identity(reference, args.timeout) if reference else None,
              "cases": []}
    # Keep a concurrently rebuilt compiler from changing halfway through a report.
    with tempfile.TemporaryDirectory(prefix="rfort-intrinsics-compiler-", ignore_cleanup_errors=True) as tmp:
        snapshot = Path(tmp) / compiler.name
        shutil.copy2(compiler, snapshot)
        if hashlib.sha256(snapshot.read_bytes()).hexdigest() != report["compiler"]["sha256"]:
            parser.error("compiler changed while snapshotting; retry with a stable executable")
        args.report = args.report.resolve()
        args.report.parent.mkdir(parents=True, exist_ok=True)
        with ThreadPoolExecutor(max_workers=args.jobs) as pool:
            results = pool.map(lambda source: probe(source, snapshot, reference, levels, args.timeout,
                                                   args.run, args.reference_arg), sources)
            for index, item in enumerate(results, 1):
                report["cases"].append(item)
                print(f"[{index}/{len(sources)}] {item['source']}: {item['status']}", flush=True)
                summarize(report)
                args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        args.report.with_suffix(".md").write_text(markdown(report), encoding="utf-8")
    print(json.dumps(report["optimization_summary"], indent=2))
    print(f"Report: {args.report}")
    accepted = {"compiled"} if not args.run else {"pass"}
    return int(args.strict and any(e["status"] not in accepted for c in report["cases"]
                                  for e in c["results"].values()))


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 Beliavsky
"""Validate and time existing rfort/gfortran executables. Never builds a compiler."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import fnmatch
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import sys
import time
import uuid

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
TOKENS = {"BENCH_SECONDS", "BENCH_CHECKSUM", "BENCH_ERROR"}


def residue_sum(n: int, offset: int, modulus: int) -> int:
    cycles, tail = divmod(n, modulus)
    return cycles * modulus * (modulus - 1) // 2 + sum(
        (i + offset) % modulus for i in range(1, tail + 1)
    )


def expected_result(reference: str, n: int, repetitions: int, seed: int) -> float:
    """Independent mathematical checks; no compiler is used as the oracle."""
    if reference == "integer_sum":
        return float(residue_sum(n, seed, 97))
    if reference == "recurrence":
        return 1.0 + (seed - 1) * 0.999999 ** n
    if reference == "pi":
        return math.pi
    if reference == "vector":
        return (0.5 + 0.25 * repetitions) * residue_sum(n, seed, 97) / 97.0
    if reference == "dot":
        period = 97 * 89
        def term(i):
            return ((i + seed) % 97) * ((i + 2 * seed) % 89)
        cycles, tail = divmod(n, period)
        numerator = cycles * sum(term(i) for i in range(1, period + 1))
        numerator += sum(term(i) for i in range(1, tail + 1))
        return (repetitions * numerator / (97.0 * 89.0)
                + ((1 + seed) % 97) / 97.0 * 0.0001
                * repetitions * (repetitions + 1) / 2.0)
    if reference == "reductions":
        # Every shipped profile covers a complete residue cycle. Seed <= 20
        # keeps the changing first element away from the extrema for these runs.
        values = [(i + seed) % 97 / 97.0 for i in range(1, min(n, 97) + 1)]
        lo, hi = min(values), max(values)
        first = ((1 + seed) % 97) / 97.0
        base = residue_sum(n, seed, 97) / 97.0
        return sum(base + k * 0.0001 + max(hi, first + k * 0.0001)
                   - min(lo, first + k * 0.0001) for k in range(1, repetitions + 1))
    if reference == "matrix":
        s = n * (n + 1) / 2.0
        return n * (s + n * seed) * s / (n + seed) ** 2 + repetitions * 0.000001 * s / (n + seed)
    if reference == "stencil":
        theta = math.pi / (n - 1)
        return seed * math.cos(theta) ** repetitions / math.tan(theta / 2.0) ** 2
    if reference == "allocation":
        return sum(residue_sum(n, k + seed, 97) for k in range(1, repetitions + 1)) / 97.0
    raise ValueError(f"unknown reference: {reference}")


def parse_output(stdout: str) -> dict:
    values = {}
    for line in stdout.splitlines():
        fields = line.split()
        if not fields or fields[0] not in TOKENS:
            continue
        key = fields[0]
        if key in values or len(fields) != 2:
            raise ValueError(f"duplicate or malformed {key}")
        try:
            value = float(fields[1].replace("D", "E").replace("d", "e"))
        except ValueError as error:
            raise ValueError(f"invalid {key}: {fields[1]}") from error
        if not math.isfinite(value):
            raise ValueError(f"nonfinite {key}")
        values[key] = value
    if set(values) != TOKENS:
        raise ValueError("missing benchmark output: " + ", ".join(sorted(TOKENS - set(values))))
    if values["BENCH_SECONDS"] < 0 or values["BENCH_ERROR"] < 0:
        raise ValueError("negative time or error")
    return values


def validate(values: dict, case: dict, profile: dict, seed: int) -> tuple[bool, float]:
    expected = expected_result(case["reference"], **profile, seed=seed)
    correct = math.isclose(values["BENCH_CHECKSUM"], expected,
                           rel_tol=case["rtol"], abs_tol=case["atol"])
    correct &= values["BENCH_ERROR"] <= case["error_limit"]
    return correct, expected


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def invoke(command: list[str], cwd: Path, timeout: float, env: dict, stdin=None) -> dict:
    start = time.perf_counter()
    try:
        result = subprocess.run(command, cwd=cwd, input=stdin, env=env,
                                capture_output=True, text=True, encoding="utf-8",
                                errors="replace", timeout=timeout)
        return dict(command=command, returncode=result.returncode, timeout=False,
                    seconds=time.perf_counter()-start, stdout=result.stdout, stderr=result.stderr)
    except subprocess.TimeoutExpired as error:
        def decoded(value):
            return value.decode("utf-8", errors="replace") if isinstance(value, bytes) else value or ""
        return dict(command=command, returncode=None, timeout=True,
                    seconds=time.perf_counter()-start, stdout=decoded(error.stdout), stderr=decoded(error.stderr))
    except OSError as error:
        return dict(command=command, returncode=None, timeout=False,
                    seconds=time.perf_counter()-start, stdout="", stderr=str(error))


def compiler_info(name: str, requested: str, output: Path, env: dict) -> dict:
    path = Path(requested).expanduser()
    found = str(path.resolve()) if path.is_file() else shutil.which(requested)
    info = dict(name=name, requested=requested, path=found, available=False)
    if not found:
        info["error"] = "compiler executable not found"
        return info
    if Path(found).suffix.lower() in (".bat", ".cmd"):
        info["error"] = "use a compiler executable, not a batch wrapper that might rebuild it"
        return info
    try:
        original = Path(found).resolve()
        # The compiler is being developed separately. Freeze its executable for
        # this run, without relocating gfortran's relative toolchain resources.
        if name == "rfort":
            snapshot = output / "toolchain" / original.name
            snapshot.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(original, snapshot)
            info["path"] = str(snapshot)
        info["sha256"] = digest(Path(info["path"]))
        info["original_path"] = str(original)
        version = invoke([info["path"], "--version"], output, 10, env)
        info["version"] = (version["stdout"] or version["stderr"]).strip()
        # Compilers outside the default pair may not support --version.
        info["available"] = version["returncode"] == 0 or name not in ("rfort", "gfortran")
        if not info["available"]:
            info["error"] = "could not query compiler version"
    except OSError as error:
        info["error"] = str(error)
    return info


# Implementations that run Fortran sources directly, without an executable.
INTERPRETERS = {"ofort"}


def named_values(pairs: list[str], option: str) -> list[tuple[str, str]]:
    result = []
    for pair in pairs:
        name, sep, value = pair.partition("=")
        if not sep or not name:
            raise ValueError(f"{option} expects NAME=VALUE, got {pair!r}")
        result.append((name, value))
    return result


# Run-time checking flags for --checks none and all. The checks differ: see
# the README. A compiler without an entry gets no flags in either mode.
CHECK_FLAGS = {
    "rfort": {"none": ["-nocheck"]},
    "gfortran": {"all": ["-fcheck=all"]},
    "ifx": {"all": ["/check:all"] if os.name == "nt" else ["-check", "all"]},
    "ifort": {"all": ["/check:all"] if os.name == "nt" else ["-check", "all"]},
    "ofort": {"none": ["--no-check-uninitialized"], "all": ["--check-uninitialized"]},
}


def compiler_flags(args, name: str) -> list[str]:
    extra = list(CHECK_FLAGS.get(name, {}).get(args.checks, []))
    extra += {"rfort": args.rfort_flag, "gfortran": args.gfortran_flag}.get(name, [])
    extra += [flag for n, flag in named_values(args.compiler_flag, "--compiler-flag") if n == name]
    if name in INTERPRETERS:
        return extra
    # rfort rejects duplicate optimization options, whereas gfortran accepts
    # them. Emit a single explicit level when the caller overrides the default.
    explicit = any(f.startswith("-O") or f == "--opt-level" or f.startswith("--opt-level=") for f in extra)
    defaults = [] if explicit else ["-O2"]
    # Intel compilers default to fast floating point, which reorders sums
    # that the other compilers evaluate as written. Use the precise model
    # unless a floating-point model is given.
    if name in ("ifx", "ifort") and not any(f.startswith(("/fp:", "-fp-model", "-ffp-model")) for f in extra):
        defaults += ["/fp:precise"] if os.name == "nt" else ["-fp-model", "precise"]
    return ([] if args.no_default_flags else defaults) + extra


def compiler_path(args, name: str) -> str:
    paths = dict(named_values(args.compiler_path, "--compiler-path"))
    if name in paths:
        return paths[name]
    return {"rfort": args.rfort, "gfortran": args.gfortran}.get(name, name)


def compiler_names(args) -> list[str]:
    """rfort, against which the others are compared, then the others."""
    return list(dict.fromkeys(["rfort", *args.compilers]))


def compile_case(case: dict, compiler: dict, profile: dict, args, output: Path, env: dict) -> dict:
    work = output / compiler["name"] / case["name"]
    work.mkdir(parents=True)
    result = dict(benchmark=case["name"], compiler=compiler["name"], profile=profile,
                  status="unavailable", directory=str(work), samples=[], warmups=[])
    if not compiler["available"]:
        result["error"] = compiler.get("error", "compiler unavailable")
        return result
    source = HERE / case["source"]
    shutil.copy2(source, work / source.name)
    shutil.copy2(HERE / "bench_support.f90", work / "bench_support.f90")
    (work / "bench_config.f90").write_text(
        "module bench_config\nimplicit none\ninteger,parameter :: n=" + str(profile["n"])
        + ", repetitions=" + str(profile["repetitions"]) + "\nend module\n", encoding="utf-8")
    flags = compiler_flags(args, compiler["name"])
    sources = ["bench_config.f90", "bench_support.f90", source.name]
    if compiler["name"] in INTERPRETERS:
        # Each run interprets the sources; there is no build step.
        result["run_command"] = [compiler["path"], *flags, *sources]
        result["status"] = "compiled"
        return result
    exe = work / ("benchmark.exe" if os.name == "nt" else "benchmark")
    command = [compiler["path"], *flags, *sources, "-o", str(exe)]
    build = invoke(command, work, args.compile_timeout, env)
    for stream in ("stdout", "stderr"):
        (work / ("compile." + stream + ".txt")).write_text(build[stream], encoding="utf-8")
    result["build"] = build
    result["executable"] = str(exe)
    if build["timeout"]:
        result["status"] = "compile_timeout"
    elif build["returncode"] != 0 or not exe.is_file():
        result["status"] = "compile_failed"
    else:
        result["status"] = "compiled"
    return result


def sample(result: dict, case: dict, args, env: dict, warmup: bool) -> None:
    command = result.get("run_command") or [result["executable"]]
    run = invoke(command, Path(result["directory"]), args.timeout, env, f"{args.seed}\n")
    result["warmups" if warmup else "samples"].append(run)
    if run["timeout"]:
        result["status"] = "run_timeout"
    elif run["returncode"] != 0:
        result["status"] = "run_failed"
    else:
        try:
            values = parse_output(run["stdout"])
            run["values"] = values
            correct, expected = validate(values, case, result["profile"], args.seed)
            run["expected_checksum"] = expected
            if not correct:
                result["status"] = "wrong_answer"
            elif values["BENCH_SECONDS"] > run["seconds"] + max(.005, run["seconds"] * .01):
                result["status"] = "invalid_output"
                run["validation_error"] = "kernel time exceeds complete process wall time; check SYSTEM_CLOCK"
        except ValueError as error:
            result["status"] = "invalid_output"
            run["validation_error"] = str(error)


def summarize(result: dict, minimum: float) -> None:
    if result["status"] != "compiled":
        return
    result["status"] = "passed"
    kernel = [r["values"]["BENCH_SECONDS"] for r in result["samples"]]
    wall = [r["seconds"] for r in result["samples"]]
    result["timing"] = dict(median=statistics.median(kernel), minimum=min(kernel), maximum=max(kernel),
                            stdev=statistics.stdev(kernel) if len(kernel)>1 else 0.0,
                            median_wall=statistics.median(wall))
    result["timing"]["reliable_duration"] = result["timing"]["median"] >= minimum and min(kernel) > 0


def markdown(report: dict) -> str:
    lines = ["# rfortran benchmark report", "", f"UTC: {report['created_utc']}",
             f"Profile: `{report['settings']['size']}`; seed: {report['settings']['seed']}; measured runs: {report['settings']['runs']}; run-time checks: {report['settings'].get('checks', 'default')}",
             "", "Kernel times exclude initialization, validation and printing. Process wall times and compilation times are in report.json.",
             "Ratios require correct results and sufficiently long measured durations; ratio > 1 means rfort is slower.",
             ""]
    names = list(dict.fromkeys(["rfort", *report["compilers"], *(r["compiler"] for r in report["results"])]))
    others = names[1:]
    lines.append("| Benchmark | " + " | ".join(
        [f"{n} status" for n in names] + [f"{n} seconds" for n in names] + [f"rfort / {n}" for n in others]) + " |")
    lines.append("|---|" + "---|" * len(names) + "---:|" * (len(names) + len(others)))
    for case in report["benchmarks"]:
        rows = {r["compiler"]: r for r in report["results"] if r["benchmark"] == case["name"]}
        def status(name):
            r = rows.get(name, {})
            s = r.get("status", "not selected")
            if s == "passed" and not r["timing"]["reliable_duration"]:
                s += " (short timing)"
            return s
        def seconds(name):
            r = rows.get(name, {})
            return f"{r['timing']['median']:.6g}" if r.get("status") == "passed" else "—"
        def ratio(name):
            a, b = rows.get("rfort", {}), rows.get(name, {})
            if all(r.get("status") == "passed" and r["timing"]["reliable_duration"] for r in (a,b)):
                return f"{a['timing']['median']/b['timing']['median']:.3f}"
            return "—"
        cells = [status(n) for n in names] + [seconds(n) for n in names] + [ratio(n) for n in others]
        lines.append(f"| {case['name']} | " + " | ".join(cells) + " |")
    lines.extend(["", "## Compilers", ""])
    for name, compiler in report["compilers"].items():
        lines.extend([f"### {name}", "", "```text", compiler.get("version", compiler.get("error", "unknown")), "```", ""])
    failures = [r for r in report["results"] if r["status"] != "passed"]
    if failures:
        lines.extend(["## Failures", ""])
        for r in failures:
            last = (r["warmups"]+r["samples"])[-1] if r["warmups"]+r["samples"] else r.get("build", {})
            error = r.get("error") or last.get("validation_error") or last.get("stderr") or last.get("stdout") or "See report.json"
            lines.extend([f"### {r['benchmark']} / {r['compiler']}: {r['status']}", "", "```text", error[:4000].strip(), "```", ""])
    return "\n".join(lines)+"\n"


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--compilers", nargs="+", default=["gfortran"], metavar="NAME",
                   help="Compilers to compare with rfort, which always runs, e.g. gfortran ifx flang ofort")
    p.add_argument("--rfort", default=str(ROOT / "target" / "release" / ("rfort.exe" if os.name == "nt" else "rfort")))
    p.add_argument("--gfortran", default="gfortran")
    p.add_argument("--compiler-path", action="append", default=[], metavar="NAME=PATH",
                   help="Executable for a compiler; otherwise NAME is found on PATH")
    p.add_argument("--rfort-flag", action="append", default=[], help="Extra flag; use --rfort-flag=-O0")
    p.add_argument("--gfortran-flag", action="append", default=[])
    p.add_argument("--compiler-flag", action="append", default=[], metavar="NAME=FLAG",
                   help="Extra flag for any compiler; use --compiler-flag=ifx=-O3")
    p.add_argument("--no-default-flags", action="store_true", help="Omit the default -O2 for all compilers")
    p.add_argument("--checks", choices=["default", "none", "all"], default="default",
                   help="Run-time checks: as each compiler normally runs, off for all, or on for all")
    p.add_argument("--size", choices=["smoke", "small", "medium", "large"], default="small")
    p.add_argument("--bench", nargs="+", default=["*"], help="Benchmark name globs (quote wildcards)")
    p.add_argument("--runs", type=int, default=5)
    p.add_argument("--warmups", type=int, default=1)
    p.add_argument("--seed", type=int, default=3, help="Runtime input seed (3 through 20)")
    p.add_argument("--timeout", type=float, default=30, help="Timeout per execution in seconds")
    p.add_argument("--compile-timeout", type=float, default=120)
    p.add_argument("--min-seconds", type=float, default=0.02, help="Minimum median kernel duration for a ratio")
    p.add_argument("--output", type=Path, help="Fresh directory for executables, sources, logs and reports")
    p.add_argument("--allow-failures", action="store_true", help="Exit zero after reporting compiler gaps/failures")
    p.add_argument("--list", action="store_true", help="List matching benchmarks without running compilers")
    return p


def main(argv=None) -> int:
    p = parser()
    args = p.parse_args(argv)
    if args.runs < 1 or args.warmups < 0 or not 3 <= args.seed <= 20:
        p.error("runs must be positive, warmups nonnegative, and seed between 3 and 20")
    if any(not math.isfinite(v) or v <= 0 for v in (args.timeout, args.compile_timeout, args.min_seconds)):
        p.error("timeouts and min-seconds must be finite and positive")
    try:
        named_values(args.compiler_path, "--compiler-path")
        named_values(args.compiler_flag, "--compiler-flag")
    except ValueError as error:
        p.error(str(error))
    manifest = json.loads((HERE / "manifest.json").read_text(encoding="utf-8"))
    cases = [c for c in manifest["benchmarks"] if any(fnmatch.fnmatchcase(c["name"], g) for g in args.bench)]
    if not cases:
        p.error("no benchmark names match --bench")
    if args.list:
        for c in cases:
            print(f"{c['name']:22} {c['description']}")
        return 0
    output = (args.output or ROOT / "target" / "benchmarks" /
              (datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")+"-"+uuid.uuid4().hex[:6])).resolve()
    if output.exists() and (not output.is_dir() or any(output.iterdir())):
        p.error("--output must be a new or empty directory; previous runs are preserved")
    output.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    threads = {k:"1" for k in ("OMP_NUM_THREADS", "OPENBLAS_NUM_THREADS", "MKL_NUM_THREADS", "VECLIB_MAXIMUM_THREADS")}
    env.update(threads)
    compilers = {n: compiler_info(n, compiler_path(args, n), output, env) for n in compiler_names(args)}
    others = [n for n in compilers if n != "rfort"]
    report = dict(schema_version=1, created_utc=datetime.now(timezone.utc).isoformat(),
                  machine=dict(platform=platform.platform(), processor=platform.processor(),
                               architecture=platform.machine(), logical_cpus=os.cpu_count(), python=sys.version),
                  settings={k:str(v) if isinstance(v,Path) else v for k,v in vars(args).items()},
                  thread_environment=threads, compilers=compilers, benchmarks=cases, results=[],
                  source_sha256={str(f.relative_to(HERE)):digest(f) for f in
                                 [HERE/"run.py",HERE/"manifest.json",HERE/"bench_support.f90",*[HERE/c["source"] for c in cases]]})
    def save():
        (output/"report.json").write_text(json.dumps(report,indent=2,allow_nan=False)+"\n",encoding="utf-8")
        (output/"summary.md").write_text(markdown(report),encoding="utf-8")
    print("Times are median kernel execution seconds (excluding startup and compilation).", flush=True)
    print(f"Run-time checks: {args.checks}.", flush=True)
    print("Ratio > 1 means rfort is slower; * marks a ratio involving a short timing.", flush=True)
    width = max(9, *(len(n) for n in compilers))
    labels = [f"rfort/{n}" for n in others]
    columns = [max(15, len(label)) for label in labels]
    print(f"{'Benchmark':22} {'Compiler':{width}} {'Status':24} {'Kernel median (s)':>17}"
          + "".join(f" {label:>{w}}" for label, w in zip(labels, columns)), flush=True)
    ratios = {n: [] for n in others}
    short_ratios = {n: 0 for n in others}
    for case in cases:
        rows = [compile_case(case,c,case["profiles"][args.size],args,output,env) for c in compilers.values()]
        report["results"].extend(rows)
        for r in rows:
            for _ in range(args.warmups):
                if r["status"] == "compiled": sample(r,case,args,env,True)
        # Alternate compiler execution order to reduce systematic ordering bias.
        for i in range(args.runs):
            for r in rows if i % 2 == 0 else reversed(rows):
                if r["status"] == "compiled": sample(r,case,args,env,False)
        for r in rows:
            summarize(r,args.min_seconds)
        by_compiler = {r['compiler']: r for r in rows}
        comparisons = []
        for name in others:
            ratio = '-'
            pair = [by_compiler.get('rfort'), by_compiler.get(name)]
            if all(r is not None and r['status'] == 'passed' and r['timing']['median'] > 0 for r in pair):
                relative = pair[0]['timing']['median'] / pair[1]['timing']['median']
                if math.isfinite(relative):
                    ratio = f"{relative:.3f}"
                    ratios[name].append(relative)
                    if not all(r['timing']['reliable_duration'] for r in pair):
                        ratio += '*'
                        short_ratios[name] += 1
            comparisons.append(ratio)
        for r in rows:
            status = r['status']
            seconds = '-'
            if status == 'passed':
                seconds = f"{r['timing']['median']:.4f}"
                if not r['timing']['reliable_duration']:
                    status += ' (short timing)'
            # The rfort row carries its ratio to each other compiler.
            shown = comparisons if r['compiler'] == 'rfort' else [''] * len(others)
            print(f"{case['name']:22} {r['compiler']:{width}} {status:24} {seconds:>17}"
                  + "".join(f" {c:>{w}}" for c, w in zip(shown, columns)), flush=True)
        save()
    summary = {}
    for name in others:
        values = ratios[name]
        if values:
            # Ratios are multiplicative, so the geometric mean is the usual average.
            summary[name] = dict(count=len(values), short_timing=short_ratios[name], median=statistics.median(values),
                                 geometric_mean=statistics.geometric_mean(values), mean=statistics.mean(values),
                                 sd=statistics.stdev(values) if len(values) > 1 else None, min=min(values), max=max(values))
    if summary:
        report["ratio_summary"] = summary
        save()
        keys = ("median", "geometric_mean", "mean", "sd", "min", "max")
        label = max(len("ratio"), *(len(f"rfort/{n}") for n in summary))
        print("\nSummary of ratios; n counts benchmarks with a ratio, * those marked *:")
        print(f"{'ratio':{label}} {'n':>3} {'*':>3}" + "".join(f" {k:>10}" for k in ("median", "geomean", "mean", "sd", "min", "max")))
        for name, stats in summary.items():
            print(f"{'rfort/' + name:{label}} {stats['count']:>3} {stats['short_timing']:>3}"
                  + "".join(f" {'-':>10}" if stats[k] is None else f" {stats[k]:>10.3f}" for k in keys))
    failed = sum(r["status"] != "passed" for r in report["results"])
    print(f"\n{len(report['results'])-failed} passed; {failed} failed/unavailable")
    print(f"Report: {output/'report.json'}\nSummary: {output/'summary.md'}")
    return 0 if args.allow_failures or not failed else 1


if __name__ == "__main__":
    raise SystemExit(main())

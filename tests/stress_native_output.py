"""Repeat fresh native builds and executions, retaining any failure without retrying it away."""
import argparse
import json
import os
import shutil
from pathlib import Path
import subprocess
import tempfile
import time

from failure_artifacts import FailureArtifacts, ROOT, digest
from run_ofort_compat import equivalent
from test_record_read import COMPILER


def positive(text):
    number = int(text)
    if number < 1:
        raise argparse.ArgumentTypeError("must be positive")
    return number


def matches(result, expected):
    return result.returncode == 0 and result.stderr == "" and equivalent(result.stdout, expected)


def invoke(recorder, command, cwd, timeout, data=""):
    return recorder.run(command, cwd=cwd, capture_output=True, text=True,
                        timeout=timeout, input=data)


def diagnose(recorder, compiler, obj, exe, cwd, expected, args, data):
    # These are evidence gathering, not retries of the failed test. The original
    # result and failing executable are never replaced by a diagnostic rebuild.
    results = []
    for _ in range(args.diagnostic_runs):
        result = invoke(recorder, [exe], cwd, args.run_timeout, data)
        results.append(matches(result, expected))
    if obj is None:
        return dict(same_executable_matches=results, executable_sha256=digest(exe))
    relinked = cwd / ("relinked.exe" if exe.suffix == ".exe" else "relinked")
    link = invoke(recorder, [compiler, "link", obj, "-o", relinked], cwd, args.compile_timeout)
    relinked_results = []
    if link.returncode == 0:
        for _ in range(args.diagnostic_runs):
            result = invoke(recorder, [relinked], cwd, args.run_timeout, data)
            relinked_results.append(matches(result, expected))
    return dict(same_executable_matches=results, relink_returncode=link.returncode,
                relinked_executable_matches=relinked_results,
                object_sha256=digest(obj), executable_sha256=digest(exe))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, nargs="?")
    parser.add_argument("--executable", type=Path, help="stress an existing binary, including a C/Rust control")
    parser.add_argument("--compiler", type=Path, default=COMPILER)
    parser.add_argument("--iterations", type=positive, default=100)
    parser.add_argument("--runs-per-build", type=positive, default=3)
    parser.add_argument("--opt-level", choices=("0", "1", "2", "3"), action="append")
    parser.add_argument("--expected-file", type=Path,
                        help="expected stdout; otherwise use the interpreter once as the oracle")
    parser.add_argument("--input-file", type=Path)
    parser.add_argument("--report", type=Path, default=ROOT / "target/native-stress/report.json")
    parser.add_argument("--failure-dir", type=Path,
                        help="override RFORT_TEST_FAILURE_DIR / target/test-failures")
    parser.add_argument("--diagnostic-runs", type=positive, default=5)
    parser.add_argument("--compile-timeout", type=positive, default=60)
    parser.add_argument("--run-timeout", type=positive, default=15)
    parser.add_argument("--debug-ir", action="store_true", help="retain original compiler IR on failure")
    args = parser.parse_args(argv)
    if (args.source is None)==(args.executable is None):
        parser.error("supply one source or --executable")
    if args.executable and (args.expected_file is None or args.opt_level or args.debug_ir):
        parser.error("--executable requires --expected-file and cannot use --opt-level/--debug-ir")
    source = args.source.resolve() if args.source else None
    existing = args.executable.resolve() if args.executable else None
    compiler, report_path = args.compiler.resolve(), args.report.resolve()
    data = args.input_file.read_text(encoding="utf-8") if args.input_file else ""
    program = source.read_text(encoding="utf-8") if source else None
    expected = args.expected_file.read_text(encoding="utf-8") if args.expected_file else None
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report = dict(source=str(source) if source else None, source_sha256=digest(source) if source else None,
                  executable=str(existing) if existing else None,
                  compiler=str(compiler) if source else None,
                  compiler_sha256=digest(compiler) if source else None, iterations=args.iterations,
                  expected_stdout=expected,
                  runs_per_build=args.runs_per_build, opt_levels=["existing"] if existing else args.opt_level or ["0", "2"],
                  started=time.time(), cases=[])

    def save():
        report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")

    save()
    for iteration in range(args.iterations):
        for opt in report["opt_levels"]:
            row = dict(iteration=iteration+1, opt_level=opt, runs=[], status="running")
            report["cases"].append(row)
            save()
            with tempfile.TemporaryDirectory(prefix="rfort-native-stress-") as tmp:
                work = Path(tmp)
                local = work / source.name if source else None
                if local:
                    local.write_text(program, encoding="utf-8")
                with FailureArtifacts(work, f"{(source or existing).stem}-O{opt}-{iteration+1}",
                                      expected=expected, input=data, root=args.failure_dir) as recorder:
                    try:
                        if existing:
                            # Execute the actual original path, retaining a byte
                            # identical image without changing the launch path.
                            shutil.copy2(existing,work/existing.name)
                            recorder.retained_paths[str(existing)]=existing.name
                        if expected is None:
                            oracle = invoke(recorder, [compiler, "run", local], work, args.run_timeout, data)
                            if oracle.returncode or oracle.stderr:
                                raise RuntimeError(f"interpreter oracle failed: {oracle.stderr}")
                            expected = oracle.stdout
                            recorder.expected = expected
                            report["expected_stdout"] = expected
                        obj = None if existing else work / "original.obj"
                        exe = existing or work / ("original.exe" if os.name == "nt" else "original")
                        if not existing:
                            command = [compiler, "-O", opt]
                            if args.debug_ir:
                                command.append("--debug-ir")
                            build = invoke(recorder, command+["emit-obj", local, obj], work, args.compile_timeout)
                            if build.returncode:
                                raise RuntimeError(f"object emission failed: {build.stderr}")
                            link = invoke(recorder, [compiler, "link", obj, "-o", exe], work, args.compile_timeout)
                            if link.returncode:
                                raise RuntimeError(f"link failed: {link.stderr}")
                        row.update(object_sha256=digest(obj) if obj else None, executable_sha256=digest(exe))
                        for _ in range(args.runs_per_build):
                            result = invoke(recorder, [exe], work, args.run_timeout, data)
                            row["runs"].append(dict(returncode=result.returncode, stdout=result.stdout,
                                                    stderr=result.stderr))
                            if not matches(result, expected):
                                row["status"] = "native_failure"
                                recorder.failed(f"native result differs: {result.returncode}, {result.stdout!r}, {result.stderr!r}")
                                row["diagnosis"] = diagnose(recorder, compiler, obj, exe, work, expected, args, data)
                                break
                        else:
                            row["status"] = "passed"
                    except (RuntimeError, OSError, subprocess.TimeoutExpired) as error:
                        recorder.failed(str(error))
                        if row.get("status")=="native_failure":
                            row["diagnostic_error"]=str(error)
                        else:
                            row.update(status="build_or_run_error", error=str(error))
                    if recorder.failures:
                        row["artifacts"] = str(recorder.path)
                save()
                if row["status"] != "passed":
                    print(f"{row['status']}: {row['artifacts']}", flush=True)
                    return 1
        print(f"iteration {iteration+1}/{args.iterations}: passed", flush=True)
    report["completed"] = time.time()
    save()
    print(f"Report: {report_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

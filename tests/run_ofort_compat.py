"""Run selected external ofort fixtures against rfort and a gfortran reference.

No ofort build or pytest hooks are run. Each executable runs in an isolated
temporary directory. Reports include source hashes and compiler identities.
"""
import argparse
from collections import Counter
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def run(command, cwd, timeout):
    try:
        p = subprocess.run([str(x) for x in command], cwd=cwd, input="",
                           capture_output=True, text=True, errors="replace", timeout=timeout)
        return dict(returncode=p.returncode, stdout=p.stdout, stderr=p.stderr)
    except subprocess.TimeoutExpired as e:
        return dict(timeout=True, stderr=str(e))
    except OSError as e:
        return dict(launch_error=True, stderr=str(e))


def equivalent(actual, expected):
    # The selection uses list-directed numeric/logical output. Ignore layout,
    # allow real roundoff, but compare integers exactly (including large kinds).
    left, right = actual.split(), expected.split()
    if len(left) != len(right):
        return False
    logical = {"T": True, ".TRUE.": True, "F": False, ".FALSE.": False}
    for a, b in zip(left, right):
        if a == b:
            continue
        if a.upper() in logical and b.upper() in logical:
            if logical[a.upper()] == logical[b.upper()]:
                continue
            return False
        if re.fullmatch(r"[+-]?\d+", a) and re.fullmatch(r"[+-]?\d+", b):
            if int(a) == int(b):
                continue
            return False
        try:
            if math.isclose(float(a.lower().replace("d", "e")),
                            float(b.lower().replace("d", "e")), rel_tol=2e-6, abs_tol=1e-7):
                continue
        except ValueError:
            pass
        return False
    return True


def failure(result, phase):
    if result.get("timeout"):
        return phase + "_timeout"
    if result.get("launch_error"):
        return phase + "_launch_error"
    code = result["returncode"]
    if code:
        # Rust panic/abort or a platform signal/Windows exception.
        if code < 0 or code == 101 or code >= 0x80000000:
            return phase + "_crash"
        return phase + "_error"
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ofort-root", required=True, type=Path)
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/release" / ("rfort.exe" if os.name == "nt" else "rfort"))
    parser.add_argument("--reference", default="gfortran")
    parser.add_argument("--selection", type=Path, default=ROOT / "tests/ofort/selection.json")
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--timeout", type=float, default=15)
    parser.add_argument("--strict", action="store_true", help="exit nonzero unless every selected probe passes")
    args = parser.parse_args()
    compiler = args.compiler.resolve()
    reference = shutil.which(args.reference)
    if not compiler.is_file() or not reference:
        parser.error("both the rfort compiler and gfortran reference must be available")
    selection = json.loads(args.selection.read_text(encoding="utf-8"))
    report = {"compiler": str(compiler), "compiler_sha256": hashlib.sha256(compiler.read_bytes()).hexdigest(),
              "reference": run([reference, "--version"], ROOT, args.timeout),
              "ofort_root": str(args.ofort_root.resolve()), "cases": []}
    for name in selection["cases"]:
        source = (args.ofort_root / "tests/cases" / name).resolve()
        item = {"source": name, "results": {}}
        report["cases"].append(item)
        if not source.is_file():
            item["status"] = "missing_source"
            continue
        item["sha256"] = hashlib.sha256(source.read_bytes()).hexdigest()
        with tempfile.TemporaryDirectory(prefix="rfort-ofort-") as tmp:
            directory = Path(tmp)
            ref_dir = directory / "reference"
            ref_dir.mkdir()
            exe = ref_dir / "program.exe"
            build = run([reference, "-O0", source, "-o", exe], ref_dir, args.timeout)
            item["reference_build"] = build
            status = failure(build, "reference_compile")
            if not status:
                result = run([exe], ref_dir, args.timeout)
                item["reference_run"] = result
                status = failure(result, "reference_run")
            if status:
                item["status"] = status
                print(name + ": " + status, flush=True)
                continue
            for opt in (0, 2):
                work = directory / f"O{opt}"
                work.mkdir()
                exe = work / "program.exe"
                build = run([compiler, "-O", str(opt), source, "-o", exe], work, args.timeout)
                entry = {"build": build}
                item["results"][f"O{opt}"] = entry
                status = failure(build, "compile")
                if not status:
                    actual = run([exe], work, args.timeout)
                    entry["run"] = actual
                    status = failure(actual, "run")
                    if not status:
                        status = "pass" if equivalent(actual["stdout"], result["stdout"]) else "wrong_output"
                        if status == "pass" and actual["stderr"]:
                            status = "unexpected_stderr"
                entry["status"] = status
            item["status"] = "pass" if all(v["status"] == "pass" for v in item["results"].values()) else "fail"
            print(name + ": " + ", ".join(k + "=" + v["status"] for k, v in item["results"].items()), flush=True)
    report["summary"] = dict(Counter(c["status"] for c in report["cases"]))
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["summary"]))
    return int(args.strict and any(c["status"] != "pass" for c in report["cases"]))


if __name__ == "__main__":
    raise SystemExit(main())

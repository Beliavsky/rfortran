"""Run selected or discovered external ofort fixtures against rfort and gfortran.

No ofort build or pytest hooks are run. Each executable runs in an isolated
temporary directory. Reports include source hashes and compiler identities.
"""
import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import fnmatch
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
    # Compare output values, not format layout. Ignore whitespace differences,
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
        if (code < 0 or code == 101 or code >= 0x80000000
                or "Program received signal " in result.get("stderr", "")):
            return phase + "_crash"
        return phase + "_error"
    return None


def source_code(text):
    """Remove comments and quoted contents before conservative discovery scans."""
    return re.sub(r"(?m)!.*$|'(?:[^']|'')*'|\"(?:[^\"]|\"\")*\"",
                  lambda m: " " * len(m[0]), text).lower()


def discover(root):
    sources = sorted((root / "tests/cases").glob("*"))
    sources = [p for p in sources if p.suffix.lower() == ".f90"]
    cases, excluded = [], []
    intrinsic_modules = {"iso_fortran_env", "iso_c_binding", "ieee_arithmetic",
                         "ieee_exceptions", "ieee_features"}
    for source in sources:
        reason = None
        if source.stem.startswith(("temp", "tmp")):
            reason = "temporary source"
        elif source.name == "xalloc_huge_stat.f90":
            reason = "requests a 40 GB allocation; resource-dependent result"
        elif source.stem.startswith(("xpartial_init", "xunset")):
            reason = "initialization probes need explicit validity/diagnostic expectations"
        elif not source.with_suffix(".out").is_file() and not source.stem.endswith("_pending"):
            reason = "no saved output or named pending compatibility probe"
        if not reason:
            code = source_code(source.read_text(encoding="utf-8", errors="replace"))
            if re.search(r"\bcall\s+(?:random_number|random_seed|cpu_time|system_clock|"
                         r"date_and_time|execute_command_line|system|sleep|exit|abort)\s*\(", code):
                reason = "randomness, timing or process-control calls need a tailored oracle"
            elif re.search(r"\b(?:get_command|get_command_argument|get_environment_variable)\b", code):
                reason = "depends on arguments or environment"
            elif re.search(r"\bread\s*(?:\(\s*(?:\*|5|input_unit)\s*[,)]|\*)", code):
                reason = "standard-input READ needs explicit input/redirection metadata"
            elif re.search(r"(?m)^\s*(?:#\s*include|include\b)", code):
                reason = "requires include/preprocessor dependencies"
            else:
                defined = set(re.findall(r"(?m)^\s*module\s+(?!procedure\b|function\b|subroutine\b)(\w+)", code))
                used = set(re.findall(r"\buse\s*(?:,\s*(?:non_)?intrinsic\s*::\s*|::\s*)?(\w+)", code))
                missing = used - defined - intrinsic_modules
                if missing:
                    reason = "requires companion module(s): " + ", ".join(sorted(missing))
        if reason:
            excluded.append({"source": source.name, "reason": reason})
        else:
            cases.append(source.name)
    return cases, {"source_count": len(sources), "candidate_count": len(cases),
                   "excluded": excluded}


def feature(name):
    stem = Path(name).stem
    groups = [
        ("allocation", r"alloc|dealloc|move_alloc"),
        ("procedure pointers", r"procedure|c_func_ptr"),
        ("pointers", r"pointer|associated|nullify"),
        ("derived types", r"derived|component|type_|typed_derived|polymorphic|pdt|class_"),
        ("character", r"character|string|scan|verify|new_line"),
        ("optional arguments", r"optional"),
        ("array operations", r"array|where|forall|reshape|shift|spread|pack|matmul|dot_product|"
                             r"minloc|maxloc|findloc|intrinsic_(?:locations|reductions|transform)|vec|subscript|overlap"),
        ("I/O and formats", r"format|print|read|write|io_|inquire|direct_access|stream|rewind|open_|unit"),
        ("procedures and modules", r"generic|external|elemental|pure|intent|interface|module|recursive|"
                                  r"sequence|associate|import|private|protected|use_|operator"),
        ("numeric and intrinsic", r"numeric|kind|bit|boz|epsilon|huge|tiny|precision|range|"
                                 r"digits|fraction|gamma|erf|exp|scale|spacing|nearest|bessel|trig|hypot|dim|aint|dprod")]
    return next((label for label, pattern in groups if re.search(pattern, stem)), "language basics")


def diagnostic_group(entry):
    status = entry["status"]
    if status != "compile_error":
        return status
    message = entry["build"].get("stderr", "").lower()
    if "unexpected " in message or "parse failed" in message:
        return "parse error"
    if "uninitialized" in message:
        return "initialization error"
    if "undefined" in message or "unknown " in message:
        return "name resolution error"
    if "unsupported" in message or "not supported" in message or "not yet" in message:
        return "unsupported feature"
    return "other compile error"


def probe(name, ofort_root, compiler, reference, timeout, repeat_reference=False):
    source = (ofort_root / "tests/cases" / name).resolve()
    item = {"source": name, "feature": feature(name), "results": {}}
    if not source.is_file():
        item["status"] = "missing_source"
        return item
    item["sha256"] = hashlib.sha256(source.read_bytes()).hexdigest()
    with tempfile.TemporaryDirectory(prefix="rfort-ofort-") as tmp:
        directory = Path(tmp)
        exe_name = "program.exe" if os.name == "nt" else "program"
        ref_dir = directory / "reference"
        ref_dir.mkdir()
        exe = ref_dir / exe_name
        build = run([reference, "-O0", source, "-o", exe], ref_dir, timeout)
        item["reference_build"] = build
        status = failure(build, "reference_compile")
        if not status:
            result = run([exe], ref_dir, timeout)
            item["reference_run"] = result
            status = failure(result, "reference_run")
            if not status and result["stderr"]:
                status = "reference_unexpected_stderr"
        if not status and repeat_reference:
            # A fresh directory prevents files from the first run changing the second.
            repeat_dir = directory / "reference-repeat"
            repeat_dir.mkdir()
            repeated = run([exe], repeat_dir, timeout)
            item["reference_repeat"] = repeated
            status = failure(repeated, "reference_run")
            if not status and (not equivalent(repeated["stdout"], result["stdout"])
                               or repeated["stderr"] != result["stderr"]):
                status = "reference_nondeterministic"
        if status:
            item["status"] = status
            return item
        for opt in (0, 2):
            work = directory / f"O{opt}"
            work.mkdir()
            exe = work / exe_name
            build = run([compiler, "-O", str(opt), source, "-o", exe], work, timeout)
            entry = {"build": build}
            item["results"][f"O{opt}"] = entry
            status = failure(build, "compile")
            if not status:
                actual = run([exe], work, timeout)
                entry["run"] = actual
                status = failure(actual, "run")
                if not status:
                    status = "pass" if equivalent(actual["stdout"], result["stdout"]) else "wrong_output"
                    if status == "pass" and actual["stderr"]:
                        status = "unexpected_stderr"
            entry["status"] = status
            entry["diagnostic_group"] = diagnostic_group(entry)
        item["status"] = "pass" if all(v["status"] == "pass" for v in item["results"].values()) else "fail"
    return item


def summarize(report):
    report["summary"] = dict(Counter(c["status"] for c in report["cases"]))
    report["optimization_summary"] = {
        opt: dict(Counter(c["results"][opt]["status"] for c in report["cases"] if opt in c["results"]))
        for opt in ("O0", "O2")}
    groups, features = Counter(), {}
    for case in report["cases"]:
        features.setdefault(case["feature"], Counter())[case["status"]] += 1
        if case["status"] == "fail":
            # Count a case once, retaining both optimization results in the report.
            entry = next(e for e in case["results"].values() if e["status"] != "pass")
            groups[entry["diagnostic_group"]] += 1
    report["failure_groups"] = dict(groups.most_common())
    report["feature_summary"] = {k: dict(v) for k, v in sorted(features.items())}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ofort-root", required=True, type=Path)
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/release" / ("rfort.exe" if os.name == "nt" else "rfort"))
    parser.add_argument("--reference", default="gfortran")
    selection_args = parser.add_mutually_exclusive_group()
    selection_args.add_argument("--selection", type=Path)
    selection_args.add_argument("--discover", action="store_true", help="inventory standalone output/pending cases, recording conservative exclusions")
    parser.add_argument("--filter", default="*", help="source filename glob, e.g. 'xcharacter*'")
    parser.add_argument("--jobs", type=int, default=1, help="independent cases to run concurrently")
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--timeout", type=float, default=15)
    parser.add_argument("--strict", action="store_true", help="exit nonzero unless every selected probe passes")
    args = parser.parse_args()
    if args.jobs < 1 or args.timeout <= 0:
        parser.error("--jobs and --timeout must be positive")
    compiler = args.compiler.resolve()
    reference = shutil.which(args.reference)
    if not compiler.is_file() or not reference:
        parser.error("both the rfort compiler and gfortran reference must be available")
    discovery = None
    if args.discover:
        cases, discovery = discover(args.ofort_root)
    else:
        selection_path = args.selection or ROOT / "tests/ofort/selection.json"
        cases = json.loads(selection_path.read_text(encoding="utf-8"))["cases"]
    cases = [name for name in cases if fnmatch.fnmatchcase(name, args.filter)]
    if not cases:
        parser.error("no cases matched the requested selection/filter")
    report = {"compiler": str(compiler), "compiler_sha256": hashlib.sha256(compiler.read_bytes()).hexdigest(),
              "reference": run([reference, "--version"], ROOT, args.timeout),
              "ofort_root": str(args.ofort_root.resolve()),
              "mode": "discovery" if args.discover else "selection",
              "filter": args.filter, "jobs": args.jobs, "timeout": args.timeout,
              "comparison": "whitespace tokens, logical spelling, numeric tolerance",
              "cases": []}
    if discovery is not None:
        report["discovery"] = discovery
        print(f"Inventory: {discovery['source_count']} sources, {discovery['candidate_count']} candidates, "
              f"{len(discovery['excluded'])} excluded; running {len(cases)}", flush=True)

    def execute(name):
        return probe(name, args.ofort_root, compiler, reference, args.timeout,
                     repeat_reference=args.discover)
    with ThreadPoolExecutor(max_workers=args.jobs) as executor:
        for index, item in enumerate(executor.map(execute, cases), 1):
            report["cases"].append(item)
            detail = ", ".join(k + "=" + v["status"] for k, v in item["results"].items()) or item["status"]
            print(f"[{index}/{len(cases)}] {item['source']}: {detail}", flush=True)
    summarize(report)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["summary"]))
    print("Failure groups: " + json.dumps(report["failure_groups"]))
    return int(args.strict and any(c["status"] != "pass" for c in report["cases"]))


if __name__ == "__main__":
    raise SystemExit(main())

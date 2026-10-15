"""Test external util.f90 procedures without rewriting their implementations.

See tests/util/README.md. This is a conservative free-form source extractor for
this utility module, not a general Fortran parser. Full-module mode is available
to check the integration that isolated tests intentionally omit.
"""
import argparse
from collections import Counter
from dataclasses import dataclass
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

from run_ofort_compat import equivalent, failure, run

ROOT = Path(__file__).resolve().parents[1]
SUITE = ROOT / "tests/util"


def code_only(text):
    """Mask strings/comments, preserving newlines and offsets, including doubled quotes."""
    result = list(text)
    quote = None
    comment = False
    i = 0
    while i < len(text):
        c = text[i]
        if c == "\n":
            comment = False
        elif comment:
            result[i] = " "
        elif quote:
            result[i] = " "
            if c == quote:
                if i + 1 < len(text) and text[i + 1] == quote:
                    result[i + 1] = " "
                    i += 1
                else:
                    quote = None
        elif c == "!":
            result[i] = " "
            comment = True
        elif c in "\"'":
            result[i] = " "
            quote = c
        i += 1
    return "".join(result).lower()


@dataclass
class Procedure:
    name: str
    kind: str
    start: int
    end: int
    parent: str | None
    source: str

    @property
    def qualified(self):
        return f"{self.parent}.{self.name}" if self.parent else self.name


class Utility:
    def __init__(self, source):
        self.source = source
        lines = source.splitlines(keepends=True)
        code = code_only(source).splitlines()
        self.contains = next(i for i, line in enumerate(code) if line.strip() == "contains")
        self.procedures = []
        stack = []
        header = re.compile(r"^\s*(?!end\b)[^!=]*?\b(function|subroutine)\s+(\w+)\s*\(")
        ending = re.compile(r"^\s*end\s*(function|subroutine)\b\s*(\w+)?\s*$")
        for i in range(self.contains + 1, len(code)):
            line = code[i]
            match = header.match(line)
            if match:
                parent = ".".join(item[1] for item in stack) or None
                stack.append((match[1], match[2], i, parent))
            elif ending.match(line) or (stack and line.strip() == "end"):
                if not stack:
                    raise ValueError(f"unmatched procedure end at line {i+1}")
                kind, name, start, parent = stack.pop()
                end = ending.match(line)
                if end and (end[1] != kind or end[2] and end[2] != name):
                    raise ValueError(f"mismatched procedure end at line {i+1}")
                self.procedures.append(Procedure(name, kind, start+1, i+1, parent,
                                                "".join(lines[start:i+1])))
        if stack:
            raise ValueError(f"unterminated procedure {stack[-1][1]}")
        self.procedures.sort(key=lambda p: p.start)
        self.top = {p.name: p for p in self.procedures if p.parent is None}
        if len(self.top) != sum(p.parent is None for p in self.procedures):
            raise ValueError("duplicate top-level procedure name")
        self.interfaces = {}
        self.specification = []
        i = 0
        while i < self.contains:
            line = code[i].strip()
            if re.match(r"interface\b", line):
                name = re.sub(r"\s+", "", line[len("interface"):])
                j = i + 1
                while j < self.contains and not re.match(r"\s*end\s*interface\b", code[j]):
                    j += 1
                if j == self.contains:
                    raise ValueError(f"unterminated interface at line {i+1}")
                body = " ".join(code[i+1:j]).replace("&", " ")
                names = []
                for match in re.finditer(r"module\s+procedure\s+(.*?)(?=module\s+procedure|$)", body):
                    names.extend(re.findall(r"\b\w+\b", match[1]))
                if not name or not names:
                    raise ValueError(f"unsupported interface at line {i+1}")
                self.interfaces[name] = names
                i = j + 1
            elif re.match(r"(?:public|private)\b", line):
                # Make selected specifics callable in isolated tests. Retain
                # original accessibility in full-module integration mode.
                while code[i].rstrip().endswith("&"):
                    i += 1
                i += 1
            else:
                self.specification.append(lines[i])
                i += 1

    def references(self, text):
        code = code_only(text).replace("&", " ")
        names = set(re.findall(r"\b(\w+)\s*\(", code))
        names.update(re.findall(r"\bcall\s+(\w+)\b", code))
        for name in self.interfaces:
            if name.startswith("operator("):
                operator = name[9:-1]
                if operator.startswith(".") and operator in code or re.search(
                        r"operator\s*\(\s*" + re.escape(operator) + r"\s*\)", code):
                    names.add(name)
        return names

    def closure(self, names, driver=""):
        pending = set(names) | self.references(driver)
        selected = set()
        while pending:
            name = pending.pop()
            if name in self.interfaces:
                pending.update(p for p in self.interfaces[name] if p not in selected)
            elif name in self.top and name not in selected:
                selected.add(name)
                pending.update(self.references(self.top[name].source))
        return selected

    def extract(self, selected):
        result = list(self.specification)
        for generic, specifics in self.interfaces.items():
            kept = [name for name in specifics if name in selected]
            if kept:
                result.append(f"interface {generic}\n")
                result.extend(f"module procedure {name}\n" for name in kept)
                result.append("end interface\n")
        result.append("contains\n")
        result.extend(p.source + "\n" for p in self.procedures if p.parent is None and p.name in selected)
        result.append("end module util_mod\n")
        return "".join(result)


def identity(path):
    return {"path": str(path.resolve()), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def write_files(directory, files):
    for name, contents in files.items():
        path = Path(name)
        if path.is_absolute() or ".." in path.parts:
            raise ValueError(f"fixture path must be relative: {name}")
        target = directory / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(contents, encoding="utf-8")


def compile_run(compiler, kind, utility, driver, work, timeout, opt=None, data="", files=None):
    work.mkdir()
    write_files(work, files or {})
    kind_path, util_path, main_path = (work / name for name in ("kind.f90", "util.f90", "driver.f90"))
    kind_path.write_text(kind, encoding="utf-8")
    util_path.write_text(utility, encoding="utf-8")
    main_path.write_text(driver, encoding="utf-8")
    exe = work / ("driver.exe" if os.name == "nt" else "driver")
    # util.f90 includes GNU intrinsics such as GETCWD. Keep gfortran's default
    # dialect, with bounds/allocation checks, rather than treating these as
    # external procedures under a strict standard mode.
    flags = ["-O" + str(opt), "-J", str(work)] if opt is not None else ["-fcheck=all"]
    built = run([compiler, *flags, kind_path, util_path, main_path, "-o", exe], work, timeout)
    outcome = failure(built, "compile")
    result = {"compile": built}
    if outcome:
        return dict(result, outcome=outcome)
    try:
        p = subprocess.run([str(exe)], cwd=work, input=data, capture_output=True,
                           text=True, errors="replace", timeout=timeout)
        executed = dict(returncode=p.returncode, stdout=p.stdout, stderr=p.stderr)
    except subprocess.TimeoutExpired:
        executed = dict(timeout=True, stderr="execution timeout")
    except OSError as e:
        executed = dict(launch_error=True, stderr=str(e))
    return dict(result, run=executed, outcome=failure(executed, "run") or "completed")


def summarize(utility, cases):
    inventory = []
    for procedure in utility.procedures:
        targeted = [c for c in cases if procedure.qualified in c["procedures"]]
        outcomes = [r["outcome"] for c in targeted for r in c.get("results", {}).values()]
        status = "untested" if not targeted else "not_run" if not outcomes else (
            "pass" if all(s == "pass" for s in outcomes) else
            "partial" if "pass" in outcomes else "blocked")
        inventory.append(dict(name=procedure.qualified, kind=procedure.kind,
                              line=procedure.start, end_line=procedure.end, status=status,
                              tested_by=[c["id"] for c in targeted],
                              included_by=[c["id"] for c in cases if
                                           (procedure.parent or procedure.name).split(".")[0] in c["included"]]))
    return {"procedure_counts": dict(Counter(p["status"] for p in inventory)),
            "case_outcomes": dict(Counter(r["outcome"] for c in cases for r in c.get("results", {}).values())),
            "procedures": inventory}


def compare_execution(actual, reference):
    if actual.get("outcome") != "completed":
        return actual["outcome"]
    return "pass" if equivalent(actual["run"]["stdout"], reference["run"]["stdout"]) else "output_mismatch"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--util-source", type=Path, default=ROOT / "util.f90")
    parser.add_argument("--kind-source", type=Path, default=ROOT / "kind.f90")
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/release" / ("rfort.exe" if os.name == "nt" else "rfort"))
    parser.add_argument("--reference", default="gfortran")
    parser.add_argument("--case", action="append", default=[], help="case name/glob (repeatable)")
    parser.add_argument("--mode", choices=["isolated", "module", "both"], default="isolated")
    parser.add_argument("--optimizations", type=int, choices=[0, 1, 2, 3], nargs="+", default=[0, 2])
    parser.add_argument("--timeout", type=float, default=90)
    parser.add_argument("--report", type=Path, default=ROOT / "target/util-coverage.json")
    parser.add_argument("--snapshot", type=Path, help="also write a compact, reviewable coverage baseline")
    parser.add_argument("--repro-dir", type=Path, default=ROOT / "target/util-repros",
                        help="retain standalone generated source groups here")
    parser.add_argument("--inventory-only", action="store_true")
    parser.add_argument("--strict", action="store_true", help="nonzero exit for any selected failure")
    args = parser.parse_args()
    for path in [args.util_source, args.kind_source]:
        if not path.is_file():
            parser.error(f"source not found: {path}; supply --util-source and --kind-source")
    utility = Utility(args.util_source.read_text(encoding="utf-8-sig"))
    kind = args.kind_source.read_text(encoding="utf-8-sig")
    definitions = json.loads((SUITE / "cases.json").read_text(encoding="utf-8"))
    cases = []
    for case in definitions:
        if args.case and not any(fnmatch.fnmatchcase(case["id"], pattern) for pattern in args.case):
            continue
        missing = set(case["procedures"]) - {p.qualified for p in utility.procedures}
        if missing:
            parser.error(f"{case['id']}: unknown procedures: {sorted(missing)}")
        driver = (SUITE / "drivers" / (case["id"] + ".f90")).read_text(encoding="utf-8")
        selected = utility.closure(case["procedures"] + case.get("dependencies", []), driver)
        cases.append(dict(case, isolated_dependencies=sorted(selected),
                          included=sorted(selected if args.mode == "isolated" else utility.top),
                          driver=driver, results={}))
    if not cases:
        parser.error("no cases selected")
    report = {"sources": {"util": identity(args.util_source), "kind": identity(args.kind_source),
                          "manifest": identity(SUITE / "cases.json")},
              "compiler": str(args.compiler.resolve()), "reference": args.reference,
              "mode": args.mode, "optimizations": args.optimizations}
    if not args.inventory_only:
        reference = shutil.which(args.reference)
        if reference is None:
            parser.error("reference compiler not found; install gfortran or supply --reference")
        if not args.compiler.is_file():
            parser.error(f"compiler not found: {args.compiler}")
        report["compiler_identity"] = identity(args.compiler)
        report["reference_version"] = run([reference, "--version"], ROOT, args.timeout)
        modes = ["isolated", "module"] if args.mode == "both" else [args.mode]
        with tempfile.TemporaryDirectory(prefix="rfort-util-") as tmp:
            for number, case in enumerate(cases):
                for mode in modes:
                    source = utility.extract(set(case["isolated_dependencies"])) if mode == "isolated" else utility.source
                    repro = args.repro_dir / case["id"] / mode
                    repro.mkdir(parents=True, exist_ok=True)
                    for filename, contents in [("kind.f90", kind), ("util.f90", source), ("driver.f90", case["driver"])]:
                        (repro / filename).write_text(contents, encoding="utf-8")
                    write_files(repro, case.get("files", {}))
                    case.setdefault("reproductions", {})[mode] = str(repro.resolve())
                    directory = Path(tmp) / f"{number}-{mode}"
                    directory.mkdir()
                    ref = compile_run(reference, kind, source, case["driver"], directory / "reference", args.timeout,
                                      files=case.get("files"))
                    # A reference that reads undefined data or prints time/random
                    # values cannot serve as a deterministic differential oracle.
                    if ref["outcome"] == "completed":
                        exe = directory / "reference" / ("driver.exe" if os.name == "nt" else "driver")
                        write_files(directory / "reference", case.get("files", {}))
                        repeated = run([exe], directory / "reference", args.timeout)
                        ref["repeat_run"] = repeated
                        if failure(repeated, "run") or not equivalent(repeated.get("stdout", ""), ref["run"]["stdout"]):
                            ref["outcome"] = "unstable"
                    for opt in args.optimizations:
                        key = f"{mode}-O{opt}"
                        if ref["outcome"] != "completed":
                            result = {"outcome": "reference_" + ref["outcome"], "reference": ref}
                        else:
                            result = compile_run(args.compiler.resolve(), kind, source, case["driver"],
                                                 directory / f"O{opt}", args.timeout, opt, files=case.get("files"))
                            result["reference"] = ref
                            result["outcome"] = compare_execution(result, ref)
                        case["results"][key] = result
                        print(f"{case['id']:30} {key:14} {result['outcome']}", flush=True)
    for case in cases:
        case["driver_sha256"] = hashlib.sha256(case.pop("driver").encode()).hexdigest()
    report["cases"] = cases
    report.update(summarize(utility, cases))
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    if args.snapshot:
        snapshot = {"source_hashes": {n: s["sha256"] for n,s in report["sources"].items()},
                    "compiler_sha256": report.get("compiler_identity", {}).get("sha256"),
                    "mode": args.mode, "optimizations": args.optimizations,
                    "procedure_counts": report["procedure_counts"], "case_outcomes": report["case_outcomes"],
                    "cases": [{"id": c["id"], "procedures": c["procedures"],
                               "driver_sha256": c["driver_sha256"],
                               "outcomes": {n:r["outcome"] for n,r in c["results"].items()}}
                              for c in cases],
                    "inventory": [{"name": p["name"], "line": p["line"], "status": p["status"],
                                   "tested_by": p["tested_by"]} for p in report["procedures"]]}
        args.snapshot.parent.mkdir(parents=True, exist_ok=True)
        args.snapshot.write_text(json.dumps(snapshot, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["procedure_counts"], sort_keys=True))
    print(f"Report: {args.report}")
    return int(args.strict and any(k != "pass" for k in report["case_outcomes"]))


if __name__ == "__main__":
    raise SystemExit(main())

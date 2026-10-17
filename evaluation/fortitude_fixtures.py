"""Run rfort parse/check and gfortran -fsyntax-only on Fortitude's Fortran fixtures.

Usage: python fortitude_run.py FORTITUDE_ROOT RFORT OUT_JSON
Files are only read and passed as paths; nothing is copied.
"""
import json, re, subprocess, sys, tempfile
from pathlib import Path

root, rfort, out = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
exts = {".f90", ".f", ".f95", ".f03", ".f08", ".for"}
files = sorted(p for p in root.rglob("*") if p.suffix.lower() in exts and "target" not in p.parts)


def first_error(text):
    for line in text.splitlines():
        s = line.strip()
        if re.match(r"(error|Error|Parse|Semantic)\b", s):
            return s[:200]
    lines = [l.strip() for l in text.splitlines() if l.strip()]
    return lines[0][:200] if lines else ""


def location(text):
    m = re.search(r"┌─ .*?:(\d+):(\d+)", text)
    return f"{m.group(1)}:{m.group(2)}" if m else ""


results = []
with tempfile.TemporaryDirectory() as tmp:
    for f in files:
        rel = f.relative_to(root).as_posix()
        g = subprocess.run(["gfortran", "-fsyntax-only", "-J", tmp, str(f)], capture_output=True, text=True, cwd=tmp)
        p = subprocess.run([rfort, "parse", str(f)], capture_output=True, text=True, encoding="utf-8", errors="replace", cwd=tmp, timeout=60)
        c = subprocess.run([rfort, "check", str(f)], capture_output=True, text=True, encoding="utf-8", errors="replace", cwd=tmp, timeout=120)
        results.append({
            "file": rel,
            "gfortran_ok": g.returncode == 0,
            "gfortran_error": "" if g.returncode == 0 else first_error(g.stderr) or g.stderr.strip()[:200],
            "parse_ok": p.returncode == 0,
            "parse_error": "" if p.returncode == 0 else first_error(p.stderr + p.stdout),
            "parse_location": "" if p.returncode == 0 else location(p.stderr + p.stdout),
            "check_ok": c.returncode == 0,
            "check_error": "" if c.returncode == 0 else first_error(c.stderr + c.stdout),
            "check_location": "" if c.returncode == 0 else location(c.stderr + c.stdout),
        })
        print(rel, results[-1]["gfortran_ok"], results[-1]["parse_ok"], results[-1]["check_ok"], flush=True)
out.write_text(json.dumps(results, indent=1), encoding="utf-8")

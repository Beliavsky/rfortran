"""Build a real Fortran shared library and call it from a Rust executable."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

from generate import generate
import json

ROOT = Path(__file__).resolve().parent


def run(command, *, cwd, env=None):
    print("+ " + subprocess.list2cmdline([str(item) for item in command]), flush=True)
    subprocess.run([str(item) for item in command], cwd=cwd, env=env, check=True, timeout=120)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--example", choices=("demo", "rnorm"), default="demo")
    parser.add_argument("--out-dir", type=Path)
    parser.add_argument("--gfortran", default="gfortran")
    parser.add_argument("--rustc", default="rustc")
    args = parser.parse_args()
    compiler, rustc = shutil.which(args.gfortran), shutil.which(args.rustc)
    if not compiler or not rustc:
        parser.error("Both gfortran and rustc must be available (or pass their paths).")
    work = (args.out_dir or ROOT.parent / "target" / ("bindings-" + args.example)).resolve()
    if work in (ROOT, ROOT.parent, ROOT / "examples"):
        parser.error("Choose a separate build directory, not a source directory.")
    work.mkdir(parents=True, exist_ok=True)
    spec = json.loads((ROOT / "examples" / (args.example + ".json")).read_text())
    for name, content in generate(spec).items():
        (work / name).write_text(content, encoding="utf-8")
    driver = "main.rs" if args.example == "demo" else "rnorm_main.rs"
    source = ROOT / "examples" / "demo.f90" if args.example == "demo" else ROOT.parent / "rnorm.f90"
    shutil.copy2(ROOT / "examples" / driver, work / "main.rs")
    library = spec["library"]
    if sys.platform == "win32":
        shared = work / (library + ".dll")
        extra = []
    elif sys.platform == "darwin":
        shared = work / ("lib" + library + ".dylib")
        extra = ["-fPIC"]
    else:
        shared = work / ("lib" + library + ".so")
        extra = ["-fPIC"]
    run([compiler, "-shared", "-O2", "-std=f2018", "-Wall", "-Wextra", "-fcheck=all",
         *extra, "-J", work, source, work / "bindings.f90", "-o", shared], cwd=work)
    executable = work / ("demo.exe" if sys.platform == "win32" else "demo")
    run([rustc, "--edition=2021", "-D", "warnings", work / "main.rs", "-L", f"native={work}", "-o", executable], cwd=work)
    env = dict(os.environ)
    # A gfortran DLL may depend on its compiler distribution's runtime DLLs.
    env["PATH"] = str(Path(compiler).parent) + os.pathsep + str(work) + os.pathsep + env.get("PATH", "")
    for variable in ("LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH"):
        env[variable] = str(work) + os.pathsep + env.get(variable, "")
    run([executable], cwd=work, env=env)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, subprocess.SubprocessError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(1)

"""The fixed, compiler-independent request contract for the playground."""
import re

MAX_BODY = 262144
MAX_SOURCE = 131072
MAX_STDIN = 32768
MAX_OUTPUT = 65536
COMPILE_SECONDS = 20
RUN_SECONDS = 3
JOB_SECONDS = 30


def validate(value):
    if not isinstance(value, dict) or set(value) - {"action", "files", "stdin", "optimization", "warnings"}:
        raise ValueError("Unknown request fields; only rfort compilation is supported.")
    action = value.get("action", "run")
    if action not in ("compile", "run"):
        raise ValueError("Action must be compile or run.")
    optimization = value.get("optimization", "0")
    if optimization not in ("0", "2"):
        raise ValueError("Optimization must be 0 or 2.")
    warnings = value.get("warnings", False)
    if type(warnings) is not bool:
        raise ValueError("Warnings must be a boolean.")
    stdin = value.get("stdin", "")
    if not isinstance(stdin, str) or len(stdin.encode("utf-8")) > MAX_STDIN:
        raise ValueError("Standard input exceeds 32 KiB.")
    files = value.get("files")
    if not isinstance(files, list) or not 1 <= len(files) <= 8:
        raise ValueError("Supply between one and eight source files, in dependency order.")
    total, seen = 0, set()
    for file in files:
        if not isinstance(file, dict) or set(file) != {"name", "content"}:
            raise ValueError("Each source needs a name and content.")
        name, content = file["name"], file["content"]
        if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z][A-Za-z0-9_-]{0,55}\.(?:f90|f95|f03|f08)", name, re.I):
            raise ValueError("Use simple Fortran filenames, such as main.f90.")
        if name.casefold() in seen:
            raise ValueError("Source filenames must be unique.")
        seen.add(name.casefold())
        if not isinstance(content, str) or "\0" in content:
            raise ValueError("Source content must be text without NUL characters.")
        total += len(content.encode("utf-8"))
    if total > MAX_SOURCE:
        raise ValueError("Sources exceed 128 KiB in total.")
    return {"action": action, "files": files, "stdin": stdin,
            "optimization": optimization, "warnings": warnings}

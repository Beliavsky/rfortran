"""Keep the original inputs, binaries and subprocess output when a check fails.

No failed command is retried by this helper. Diagnostic reruns must be recorded
as separate commands so that a later success cannot replace the original result.
"""
import hashlib
from functools import lru_cache
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parent.parent


@lru_cache(maxsize=256)
def _digest(path, size, mtime):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def digest(path):
    path = Path(path)
    if not path.is_file():
        return None
    stat = path.stat()
    return _digest(str(path.resolve()), stat.st_size, stat.st_mtime_ns)


class FailureArtifacts:
    def __init__(self, directory, label, expected=None, input="", root=None):
        self.directory = Path(directory).resolve()
        self.root = Path(root or os.environ.get("RFORT_TEST_FAILURE_DIR", ROOT / "target/test-failures")).resolve()
        self.label = label
        label = re.sub(r"[^\w.-]+", "_", label)[-120:]
        self.path = self.root / (time.strftime("%Y%m%d-%H%M%S-") + label + "-" + uuid.uuid4().hex[:8])
        self.expected = expected
        self.input = input
        self.commands = []
        self.failures = []
        self.failure_events = []
        self.retained_paths = {}

    def __enter__(self):
        return self

    def run(self, command, **kwargs):
        row = dict(command=[os.fspath(a) for a in command],
                   cwd=str(Path(kwargs.get("cwd") or Path.cwd()).resolve()),
                   timeout=kwargs.get("timeout"), input=kwargs.get("input"),
                   started=time.time())
        self.commands.append(row)
        start = time.monotonic()
        try:
            result = subprocess.run(command, **kwargs)
            row.update(returncode=result.returncode, stdout=result.stdout, stderr=result.stderr)
            # Record the image at execution time, before a later build can
            # overwrite it. The artifact directory retains both native variants.
            exe = Path(command[0])
            if not exe.is_absolute():
                local=Path(row["cwd"])/exe
                resolved=str(local) if local.is_file() else shutil.which(os.fspath(exe))
                exe=Path(resolved) if resolved else exe
            if exe.is_file():
                row["executable"] = str(exe.resolve())
                try:
                    row["executable_sha256"] = digest(exe)
                except OSError as error:
                    row["executable_hash_error"] = repr(error)
            return result
        except (OSError, subprocess.TimeoutExpired) as error:
            row.update(exception=repr(error), stdout=getattr(error, "stdout", None),
                       stderr=getattr(error, "stderr", None))
            self.failed(str(error))
            if hasattr(error, "add_note"):
                error.add_note(f"Failure artifacts: {self.path}")
            raise
        finally:
            row["elapsed_seconds"] = time.monotonic() - start

    def failed(self, message):
        self.failures.append(str(message))
        self.failure_events.append(dict(command_index=len(self.commands)-1, message=str(message)))
        return f"{message}\nFailure artifacts: {self.path}"

    def __exit__(self, kind, error, traceback):
        if error is not None:
            if not self.failures:
                self.failed(repr(error))
            if hasattr(error, "add_note"):
                error.add_note(f"Failure artifacts: {self.path}")
        if self.failures:
            self.save()
        return False

    def save(self):
        self.root.mkdir(parents=True, exist_ok=True)
        shutil.copytree(self.directory, self.path, dirs_exist_ok=True)
        (self.path / "stdin.txt").write_bytes(self.input.encode("utf-8"))
        working_directories = {}
        temporary = Path(tempfile.gettempdir()).resolve()
        for command in self.commands:
            cwd = Path(command["cwd"])
            # Pointer tests also have an outer temporary cwd, holding module
            # interfaces and data files. Preserve it before test cleanup.
            if str(cwd) in working_directories or cwd == temporary or temporary not in cwd.parents:
                continue
            if cwd == self.directory or self.directory in cwd.parents or cwd in self.directory.parents:
                continue
            name = f"working-directory-{len(working_directories)}"
            def ignore(folder, entries):
                return [n for n in entries if Path(folder,n).resolve()==self.root]
            shutil.copytree(cwd, self.path/name, ignore=ignore)
            working_directories[str(cwd)] = name
        def text(value):
            return value.decode("utf-8", errors="replace") if isinstance(value, bytes) else value
        def relocate(value):
            # Preserve original paths separately; rerun commands refer to the
            # retained copy when their input/output was in the temporary case.
            if value in self.retained_paths:
                return str(self.path / self.retained_paths[value])
            original=str(self.directory)
            if value==original or value.startswith(original+os.sep):
                return str(self.path)+value[len(original):]
            for original,name in working_directories.items():
                if value==original or value.startswith(original+os.sep):
                    return str(self.path/name)+value[len(original):]
            return value
        rows = []
        for i, row in enumerate(self.commands):
            row = dict(row)
            row["retained_command"] = [relocate(a) for a in row["command"]]
            row["retained_cwd"] = relocate(row["cwd"])
            for stream in ("stdout", "stderr"):
                value = row.pop(stream, None)
                if value is not None:
                    filename = f"command-{i:03d}.{stream}.txt"
                    (self.path / filename).write_bytes(value if isinstance(value, bytes) else value.encode("utf-8"))
                    row[stream + "_file"] = filename
            row["input"] = text(row["input"])
            rows.append(row)
        files = {str(p.relative_to(self.path)): digest(p) for p in self.path.rglob("*") if p.is_file()}
        report = dict(label=self.label, original_directory=str(self.directory),
                      artifact_directory=str(self.path), expected=self.expected, input=self.input,
                      failures=self.failures, failure_events=self.failure_events, commands=rows, files_sha256=files,
                      working_directories=working_directories,
                      platform=sys.platform, python=sys.version,
                      environment={k: os.environ[k] for k in (
                          "RFORT_TEST_COMPILER", "RFORT_LINKER", "RFORT_LIB_PATH", "RFORT_CC") if k in os.environ})
        (self.path / "failure.json").write_text(json.dumps(report, indent=2), encoding="utf-8")

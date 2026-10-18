"""Read rfort's serialized AST; never parse Fortran independently or build rfort."""
from __future__ import annotations

import ast
from dataclasses import dataclass, field
import functools
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


class Unsupported(ValueError):
    def __init__(self, line, message):
        self.line = line
        super().__init__(message)


class FrontendError(ValueError):
    """Compiler diagnostics or failure to obtain a compatible compiler AST."""


@dataclass
class Node:
    kind: str
    line: int
    data: object = None
    body: list = field(default_factory=list)
    otherwise: list = field(default_factory=list)


def find_compiler(explicit=None):
    candidate = explicit or os.environ.get("RFORT")
    if candidate:
        found = shutil.which(str(candidate))
        if not found:
            raise FrontendError(f"rfort executable not found: {candidate}")
        return Path(found).resolve()
    root = Path(__file__).resolve().parent.parent
    binary = "rfort.exe" if os.name == "nt" else "rfort"
    local = root / "target" / "release" / binary
    if local.is_file():
        return local
    found = shutil.which(binary)
    if found:
        return Path(found).resolve()
    raise FrontendError("An existing rfort executable is required. Pass --rfort PATH or set RFORT; this tool never builds the compiler.")


@functools.lru_cache(maxsize=4)
def snapshot_compiler(path, modified, size):
    # Keep a stable executable for parse/check without holding the build output open.
    directory = tempfile.TemporaryDirectory(prefix="rfort-rust-frontend-")
    copied = Path(directory.name) / Path(path).name
    shutil.copy2(path, copied)
    return directory, copied


class Frontend:
    def __init__(self, source, filename="input.f90", compiler=None):
        self.source = source
        self.filename = str(filename)
        selected = find_compiler(compiler)
        metadata = selected.stat()
        self.owner, self.compiler = snapshot_compiler(str(selected), metadata.st_mtime_ns, metadata.st_size)
        self.raw = source.encode("utf-8")

    def command(self, args, directory, snapshot):
        try:
            result = subprocess.run([str(self.compiler), *map(str, args)], cwd=directory,
                                    capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120)
        except (OSError, subprocess.SubprocessError) as error:
            raise FrontendError(f"rfort frontend failed: {error}") from error
        if result.returncode:
            diagnostics = (result.stderr + result.stdout).strip().replace(str(snapshot), self.filename)
            raise FrontendError(f"rfort {args[0]} failed:\n{diagnostics}")
        return result

    def process(self, emit):
        with tempfile.TemporaryDirectory(prefix="rfort-rust-source-") as directory:
            work = Path(directory)
            snapshot = work / "input.f90"
            snapshot.write_bytes(self.raw)  # Preserve exact UTF-8 offsets and newline bytes.
            exported = work / "ast.json"
            self.command(["parse", snapshot, exported], work, snapshot)
            try:
                program = json.loads(exported.read_text(encoding="utf-8"))
                if not isinstance(program, dict) or not isinstance(program.get("body"), list):
                    raise FrontendError("unsupported rfort AST schema: missing Program.body")
                body = self.program(program)
            except (KeyError, TypeError, IndexError, json.JSONDecodeError) as error:
                raise FrontendError(f"unsupported rfort AST schema: {error}") from error
            # Check backend capabilities before asking the compiler to run lowerings
            # for constructs that this Rust emitter cannot yet represent.
            generated = emit(body)
            self.command(["check", snapshot], work, snapshot)
            return generated

    def line(self, offset):
        if not isinstance(offset, int) or not 0 <= offset <= len(self.raw):
            raise FrontendError("invalid source offset in rfort AST")
        return self.raw.count(b"\n", 0, offset) + 1

    def program(self, program):
        if program.get("contains"):
            raise Unsupported(1, "unsupported Rust backend feature: contained procedures")
        if not any(isinstance(item, dict) and item.get("Implicit") == "None" for item in program["body"]):
            raise Unsupported(1, "this Rust backend currently requires IMPLICIT NONE")
        return self.statements(program["body"])

    def typ(self, value, line):
        if value in ("Real", "DoublePrecision", "Logical"):
            return {"Real":"f32", "DoublePrecision":"f64", "Logical":"bool"}[value]
        if isinstance(value, dict) and value.get("Integer", "missing") in (None, 4):
            return "i32"
        raise Unsupported(line, f"unsupported Rust backend type: {value}")

    def expr(self, value, line):
        if not isinstance(value, dict) or len(value) != 1:
            raise FrontendError("unsupported rfort expression schema")
        tag, data = next(iter(value.items()))
        if tag == "Str":
            return ast.Constant(data)
        if tag == "IntLit":
            try:
                if "_" in data:
                    raise ValueError()
                return ast.Constant(int(data))
            except ValueError:
                raise Unsupported(line, "integer kind suffixes are not yet supported by the Rust backend") from None
        if tag == "RealLit":
            if "_" in data:
                raise Unsupported(line, "real kind suffixes are not yet supported by the Rust backend")
            typ = "f64" if "d" in data.lower() else "f32"
            return ast.Call(func=ast.Name(id="_rf_" + typ), args=[ast.Constant(data.lower().replace("d", "e"))], keywords=[])
        if tag == "Logical":
            return ast.Constant(data)
        if tag == "Ident":
            return ast.Name(id="_rf_name_" + data.lower())
        if tag == "Call":
            return ast.Call(func=ast.Name(id="_rf_name_" + data[0].lower()), args=[self.expr(item, line) for item in data[1]], keywords=[])
        if tag == "Bin":
            operator, left, right = data
            a, b = self.expr(left, line), self.expr(right, line)
            arithmetic = {"Add":ast.Add, "Sub":ast.Sub, "Mul":ast.Mult, "Div":ast.Div, "Pow":ast.Pow}
            comparison = {"Eq":ast.Eq, "Ne":ast.NotEq, "Lt":ast.Lt, "Le":ast.LtE, "Gt":ast.Gt, "Ge":ast.GtE}
            if operator in arithmetic:
                return ast.BinOp(left=a, op=arithmetic[operator](), right=b)
            if operator in comparison:
                return ast.Compare(left=a, ops=[comparison[operator]()], comparators=[b])
            if operator in ("And", "Or"):
                return ast.BoolOp(op=ast.And() if operator == "And" else ast.Or(), values=[a,b])
        if tag == "Un":
            operator, operand = data
            if operator in ("Neg", "Not"):
                return ast.UnaryOp(op=ast.USub() if operator == "Neg" else ast.Not(), operand=self.expr(operand,line))
        raise Unsupported(line, f"unsupported Rust backend expression: {tag}")

    def statements(self, items, inherited_line=1):
        nodes, line = [], inherited_line
        for statement in items:
            if not isinstance(statement, dict) or len(statement) != 1:
                raise FrontendError("unsupported rfort statement schema")
            tag, data = next(iter(statement.items()))
            if tag == "SourceLocation":
                line = self.line(data)
                continue
            if tag == "Implicit":
                if data != "None":
                    raise Unsupported(line, "only IMPLICIT NONE is supported by the Rust backend")
                continue
            if tag == "DummyIntent":
                if data["intent"] != "parameter":
                    raise Unsupported(line, f"unsupported declaration attribute: {data['intent']}")
                continue
            if tag == "VarDecl":
                nodes.append(Node("decl",line,(self.typ(data["kind"],line),False,[(name.lower(),None) for name in data["names"]])))
            elif tag == "InitializedDecl":
                attrs = [item.lower() for item in data["attributes"]]
                if set(attrs) - {"parameter", "save"}:
                    raise Unsupported(line, f"unsupported declaration attributes: {attrs}")
                entities = [(name.lower(),self.expr(value,line) if value is not None else None) for name,value in data["entities"]]
                nodes.append(Node("decl",line,(self.typ(data["kind"],line),"parameter" in attrs,entities)))
            elif tag == "Assign":
                nodes.append(Node("assign",line,(data["name"].lower(),self.expr(data["value"],line))))
            elif tag == "Print":
                nodes.append(Node("print",line,[self.expr(item,line) for item in data["items"]]))
            elif tag == "If":
                nodes.append(Node("if",line,self.expr(data["cond"],line),self.statements(data["then_body"],line),self.statements(data["else_body"] or [],line)))
            elif tag == "Do":
                bounds = [self.expr(data[key],line) for key in ("start","end")]
                nodes.append(Node("do",line,(data["var"].lower(),bounds),self.statements(data["body"],line)))
            elif tag == "Block":
                nested = data["body"]
                first = next((item for item in nested if "SourceLocation" not in item), {})
                header = first.get("CallSub", {})
                if header.get("name") != "@do_step" or len(header.get("args",[])) != 4 or "Ident" not in header["args"][0]:
                    raise Unsupported(line, "unsupported Rust backend statement: Block")
                index = nested.index(first)
                bounds = [self.expr(arg,line) for arg in header["args"][1:]]
                nodes.append(Node("do",line,(header["args"][0]["Ident"].lower(),bounds),self.statements(nested[index+1:],line)))
            else:
                raise Unsupported(line, f"unsupported Rust backend statement: {tag}")
        return nodes

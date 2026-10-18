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
    def __init__(self, line, message, filename=None):
        self.line = line
        self.filename = filename
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
    def __init__(self, source, filename="input.f90", compiler=None, locations=None):
        self.source = source
        self.filename = str(filename)
        selected = find_compiler(compiler)
        metadata = selected.stat()
        self.owner, self.compiler = snapshot_compiler(str(selected), metadata.st_mtime_ns, metadata.st_size)
        self.raw = source.encode("utf-8")
        self.locations = locations

    def command(self, args, directory, snapshot, *, filename=None):
        try:
            result = subprocess.run([str(self.compiler), *map(str, args)], cwd=directory,
                                    capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120)
        except (OSError, subprocess.SubprocessError) as error:
            raise FrontendError(f"rfort frontend failed: {error}") from error
        if result.returncode:
            diagnostics = (result.stderr + result.stdout).strip()
            if filename is None and getattr(self,"locations",None):
                diagnostics = self.locations.diagnostics(diagnostics,snapshot)
            else:
                diagnostics = diagnostics.replace(str(snapshot), filename or self.filename)
            raise FrontendError(f"rfort {args[0]} failed:\n{diagnostics}")
        return result

    def process(self, emit):
        with tempfile.TemporaryDirectory(prefix="rfort-rust-source-") as directory:
            work = Path(directory)
            snapshot = work / "input.f90"
            snapshot.write_bytes(self.raw)  # Preserve exact UTF-8 offsets and newline bytes.
            exported = work / "ast.json"
            if getattr(self,"locations",None):
                # Parse each file independently before joining. This validates
                # complete source units and stops continuations crossing files.
                for index,(name,text) in enumerate(self.locations.parts):
                    part = work / f"part-{index}.f90"
                    part.write_bytes(text.encode("utf-8"))
                    parsed = work / f"part-{index}.json"
                    self.command(["parse",part,parsed],work,part,filename=name)
                    if index < len(self.locations.parts)-1:
                        unit = json.loads(parsed.read_text(encoding="utf-8"))
                        if unit.get("contains") or any(next(iter(item)) not in ("Module","SourceLocation") for item in unit["body"]):
                            raise FrontendError(f"{name}: only complete module definitions may precede the final main-program source")
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
        main_line = self.locations.starts[-1] if getattr(self,"locations",None) else 1
        if program.get("contains"):
            raise Unsupported(main_line, "unsupported Rust backend feature: contained procedures")
        if not any(isinstance(item, dict) and item.get("Implicit") == "None" for item in program["body"]):
            raise Unsupported(main_line, "this Rust backend currently requires IMPLICIT NONE")
        return self.statements(program["body"])

    def typ(self, value, line):
        if value in ("Real", "DoublePrecision", "Logical"):
            return {"Real":"f32", "DoublePrecision":"f64", "Logical":"bool"}[value]
        if isinstance(value, dict) and value.get("Integer", "missing") in (None, 4):
            return "i32"
        raise Unsupported(line, f"unsupported Rust backend type: {value}")

    def module(self, data, line):
        if not any(item.get("Implicit")=="None" for item in data["body"]):
            raise Unsupported(line,"modules currently require IMPLICIT NONE")
        body, access, default = [], {}, True
        for item in data["body"]:
            if "SourceLocation" in item:
                line = self.line(item["SourceLocation"])
            elif "Accessibility" in item:
                visibility = item["Accessibility"]
                if visibility["names"]:
                    access.update((name.lower(),visibility["public"]) for name in visibility["names"])
                else:
                    default = visibility["public"]
            elif "Function" in item or "Subroutine" in item:
                kind = "Function" if "Function" in item else "Subroutine"
                body.append(self.procedure(kind,item[kind],line))
            else:
                body += self.statements([item],line)
        return Node("module",line,{"name":data["name"].lower(),"access":access,"public":default},body)

    def procedure(self, kind, data, line):
        params = [name.lower() for name in data["params"]]
        intents, declarations, filtered = {}, {}, []
        for item in data["body"]:
            if "SourceLocation" in item and not filtered:
                line = self.line(item["SourceLocation"])
            if "DummyIntent" in item and item["DummyIntent"]["name"].lower() in params:
                attr = item["DummyIntent"]
                name,intent = attr["name"].lower(),attr["intent"].replace(" ","").lower()
                if intent not in ("intent(in)","intent(out)","intent(inout)"):
                    raise Unsupported(line,f"unsupported dummy argument attribute: {intent}")
                intents[name] = intent[7:-1]
                continue
            if "InitializedDecl" in item:
                declaration = item["InitializedDecl"]
                attrs = [attr.lower() for attr in declaration["attributes"]]
                if "parameter" not in attrs and ("save" in attrs or any(value is not None for _,value in declaration["entities"])):
                    raise Unsupported(line,"initialized/SAVE procedure locals are not yet supported")
            filtered.append(item)
        nodes = self.statements(filtered,line)
        body = []
        for node in nodes:
            if node.kind == "decl":
                typ,parameter,items = node.data
                kept = []
                for name,initializer in items:
                    if name in declarations:
                        if declarations[name] != typ or initializer is not None:
                            raise Unsupported(node.line,f"conflicting declaration `{name}`")
                        # The parser can emit a typed RESULT declaration twice.
                        continue
                    declarations[name] = typ
                    if name in params:
                        if initializer is not None or parameter:
                            raise Unsupported(node.line,"dummy arguments cannot have initializers")
                    else:
                        kept.append((name,initializer))
                if kept:
                    body.append(Node("decl",node.line,(typ,parameter,kept)))
            else:
                body.append(node)
        for name in params:
            if name not in declarations or name not in intents:
                raise Unsupported(line,f"scalar dummy `{name}` currently requires an explicit type and INTENT")
        if kind=="Function" and any(intents[name]!="in" for name in params):
            raise Unsupported(line,"function dummy arguments currently require INTENT(IN)")
        result_type = self.typ(data["return_type"],line) if kind=="Function" else None
        return Node("procedure",line,{"name":data["name"].lower(),"params":[(name,declarations[name],intents[name]) for name in params],"result":result_type},body)

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
        if tag == "Index":
            return ast.Call(func=ast.Name(id="_rf_name_"+data[0].lower()),args=[self.expr(item,line) for item in data[1]],keywords=[])
        if tag == "Section":
            args = [self.expr(data[key],line) if data[key] is not None else ast.Constant(None) for key in ("lower","upper","stride")]
            return ast.Call(func=ast.Name(id="_rf_section_"+data["name"].lower()),args=args,keywords=[])
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
            elif tag == "Module":
                nodes.append(self.module(data,line))
            elif tag == "Use":
                nodes.append(Node("use",line,data))
            elif tag == "Return":
                nodes.append(Node("return",line,self.expr(data,line) if data is not None else None))
            elif tag == "ArrayDecl":
                if len(data["dims"]) != 1:
                    raise Unsupported(line,"only rank-one numeric arrays are currently supported")
                nodes.append(Node("array_decl",line,(self.typ(data["kind"],line),data["name"].lower(),self.expr(data["dims"][0],line))))
            elif tag == "InitializedDecl":
                attrs = [item.lower() for item in data["attributes"]]
                if set(attrs) - {"parameter", "save"}:
                    raise Unsupported(line, f"unsupported declaration attributes: {attrs}")
                entities = [(name.lower(),self.expr(value,line) if value is not None else None) for name,value in data["entities"]]
                nodes.append(Node("decl",line,(self.typ(data["kind"],line),"parameter" in attrs,entities)))
            elif tag == "Assign":
                nodes.append(Node("assign",line,(data["name"].lower(),self.expr(data["value"],line))))
            elif tag == "AssignIndex":
                if len(data["indices"]) != 1:
                    raise Unsupported(line,"only rank-one element assignments are supported")
                nodes.append(Node("assign_index",line,(data["name"].lower(),self.expr(data["indices"][0],line),self.expr(data["value"],line))))
            elif tag == "Print":
                nodes.append(Node("print",line,[self.expr(item,line) for item in data["items"]]))
            elif tag == "CallSub":
                name = data["name"]
                if name.lower()=="random_number":
                    nodes.append(Node("random_number",line,[self.expr(item,line) for item in data["args"]]))
                    continue
                if name != "@formatted_print":
                    nodes.append(Node("call",line,(name.lower(),[self.expr(item,line) for item in data["args"]])))
                    continue
                args = data["args"]
                if not args or not isinstance(args[0], dict) or "Str" not in args[0]:
                    raise Unsupported(line, "formatted PRINT currently requires a literal format")
                nodes.append(Node("formatted_print",line,(args[0]["Str"],[self.expr(item,line) for item in args[1:]])))
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

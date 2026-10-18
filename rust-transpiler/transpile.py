"""A partial Rust backend using the existing rfort parser and semantic checker."""
from __future__ import annotations

import argparse
import ast
from compiler_frontend import Frontend, FrontendError, Unsupported
from dataclasses import dataclass
import math
from pathlib import Path
import re
import struct
import subprocess
import sys


@dataclass
class Value:
    code: str
    typ: str


NUMERIC = {"i32", "f32", "f64"}
FLOAT = {"f32", "f64"}


def rust_string(text):
    return '"' + "".join('\\"' if c == '"' else "\\\\" if c == "\\" else f"\\u{{{ord(c):x}}}" if ord(c) < 32 or ord(c) == 127 else c for c in text) + '"'


class Emitter:
    def __init__(self):
        self.symbols = {}
        self.parameters = set()
        self.loop_id = 0

    @staticmethod
    def rust_name(name):
        # A prefix avoids Rust keywords without excluding valid Fortran identifiers.
        return "v_" + name

    def require(self, condition, line, message):
        if not condition:
            raise Unsupported(line, message)

    def variable(self, name, line):
        self.require(name in self.symbols, line, f"undeclared variable `{name}`")
        return self.symbols[name]

    def cast(self, value, typ, line):
        if value.typ == typ:
            return value
        self.require(value.typ in NUMERIC and typ in NUMERIC, line, "cannot convert between LOGICAL and numeric values")
        if typ == "i32":
            return Value(f"rf_to_i32(({value.code}) as f64)", typ)
        return Value(f"(({value.code}) as {typ})", typ)

    def common(self, left, right, line):
        self.require(left.typ in NUMERIC and right.typ in NUMERIC, line, "numeric operands required")
        typ = "f64" if "f64" in (left.typ, right.typ) else "f32" if "f32" in (left.typ, right.typ) else "i32"
        return self.cast(left, typ, line), self.cast(right, typ, line), typ

    def expr(self, expression, initialized, line, constant=False):
        return self.emit_expr(expression, initialized, line, constant)

    def emit_expr(self, node, initialized, line, constant):
        visit = lambda value: self.emit_expr(value, initialized, line, constant)
        if isinstance(node, ast.Constant):
            if type(node.value) is bool:
                return Value("true" if node.value else "false", "bool")
            if type(node.value) is int:
                self.require(0 <= node.value <= 2147483647, line, "integer literal is outside the supported default INTEGER range")
                return Value(str(node.value) + "i32", "i32")
            raise Unsupported(line, "unsupported literal")
        if isinstance(node, ast.Name):
            name = node.id.removeprefix("_rf_name_")
            typ = self.variable(name, line)
            self.require(name in initialized, line, f"read of `{name}` before definite assignment")
            self.require(not constant or name in self.parameters, line, "PARAMETER initializer may reference only named constants")
            return Value(self.rust_name(name), typ)
        if isinstance(node, ast.UnaryOp):
            value = visit(node.operand)
            if isinstance(node.op, ast.Not):
                self.require(value.typ == "bool", line, ".NOT. requires LOGICAL")
                return Value(f"(!({value.code}))", "bool")
            self.require(value.typ in NUMERIC, line, "unary sign requires a number")
            if isinstance(node.op, ast.UAdd):
                return value
            if isinstance(node.op, ast.USub):
                code = f'({value.code}).checked_neg().expect("INTEGER negation overflow")' if value.typ == "i32" else f"(-({value.code}))"
                return Value(code, value.typ)
        if isinstance(node, ast.BinOp):
            left, right = visit(node.left), visit(node.right)
            if isinstance(node.op, ast.Pow) and right.typ == "i32":
                self.require(left.typ in NUMERIC, line, "numeric base required")
                code = f"rf_ipow({left.code}, {right.code})" if left.typ == "i32" else f"({left.code}).powi({right.code})"
                return Value(code, left.typ)
            left, right, typ = self.common(left, right, line)
            if isinstance(node.op, ast.Pow):
                return Value(f"({left.code}).powf({right.code})", typ)
            operators = {ast.Add:("+", "checked_add"), ast.Sub:("-", "checked_sub"),
                         ast.Mult:("*", "checked_mul"), ast.Div:("/", "checked_div")}
            self.require(type(node.op) in operators, line, "unsupported arithmetic operator")
            op, checked = operators[type(node.op)]
            code = f'({left.code}).{checked}({right.code}).expect("INTEGER overflow or division by zero")' if typ == "i32" else f"(({left.code}) {op} ({right.code}))"
            return Value(code, typ)
        if isinstance(node, ast.BoolOp):
            values = [visit(value) for value in node.values]
            self.require(all(value.typ == "bool" for value in values), line, ".AND./.OR. require LOGICAL operands")
            operator = " && " if isinstance(node.op, ast.And) else " || "
            return Value("(" + operator.join(f"({value.code})" for value in values) + ")", "bool")
        if isinstance(node, ast.Compare):
            self.require(len(node.ops) == 1, line, "chained comparisons are not supported")
            left, right = visit(node.left), visit(node.comparators[0])
            self.require(left.typ != "bool" and right.typ != "bool", line, "LOGICAL comparisons require .EQV./.NEQV., which are not yet supported")
            left, right, _ = self.common(left, right, line)
            operators = {ast.Eq:"==", ast.NotEq:"!=", ast.Lt:"<", ast.LtE:"<=", ast.Gt:">", ast.GtE:">="}
            self.require(type(node.ops[0]) in operators, line, "unsupported comparison")
            return Value(f"(({left.code}) {operators[type(node.ops[0])]} ({right.code}))", "bool")
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and not node.keywords:
            name = node.func.id
            if name in ("_rf_f32", "_rf_f64"):
                self.require(len(node.args) == 1 and isinstance(node.args[0], ast.Constant) and isinstance(node.args[0].value, str), line, "invalid literal")
                number = float(node.args[0].value)
                typ = name[-3:]
                try:
                    finite = math.isfinite(struct.unpack("f", struct.pack("f", number))[0]) if typ == "f32" else math.isfinite(number)
                except OverflowError:
                    finite = False
                self.require(finite, line, "real literal is outside the supported kind range")
                literal = node.args[0].value
                if literal.startswith("."):
                    literal = "0" + literal
                literal = re.sub(r"\.(?=e|$)", ".0", literal)
                return Value(literal + typ, typ)
            name = name.removeprefix("_rf_name_")
            self.require(name not in self.symbols, line, f"scalar variable `{name}` cannot be called as a procedure")
            args = [visit(arg) for arg in node.args]
            if name in ("real", "dble", "int"):
                self.require(len(args) == 1, line, "conversion intrinsics currently accept one positional argument")
                return self.cast(args[0], {"real":"f32", "dble":"f64", "int":"i32"}[name], line)
            if name in ("min", "max"):
                self.require(len(args) >= 2 and all(v.typ == args[0].typ and v.typ in NUMERIC for v in args), line, "MIN/MAX need at least two numeric arguments of the same type and kind")
                code = args[0].code
                for value in args[1:]:
                    code = f"({code}).{name}({value.code})"
                return Value(code, args[0].typ)
            if name == "mod":
                self.require(len(args) == 2 and args[0].typ == args[1].typ and args[0].typ in NUMERIC, line, "MOD requires two numeric arguments of the same type and kind")
                left, right = args
                code = f'({left.code}).checked_rem({right.code}).expect("invalid INTEGER MOD")' if left.typ == "i32" else f"(({left.code}) % ({right.code}))"
                return Value(code, left.typ)
            unary = {"sqrt", "exp", "log", "log10", "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "abs"}
            if name in unary:
                self.require(len(args) == 1, line, name.upper() + " requires one argument")
                value = args[0]
                self.require(value.typ in NUMERIC if name == "abs" else value.typ in FLOAT, line, name.upper() + " has an unsupported argument type")
                method = "ln" if name == "log" else name
                code = f'({value.code}).checked_abs().expect("INTEGER ABS overflow")' if name == "abs" and value.typ == "i32" else f"({value.code}).{method}()"
                return Value(code, value.typ)
            raise Unsupported(line, f"unsupported intrinsic or procedure `{name}`")
        raise Unsupported(line, "unsupported scalar expression")

    def block(self, nodes, initialized, *, level=1, active_loops=frozenset(), declarations=False):
        lines, state = [], set(initialized)
        indent = "    " * level
        executable = not declarations
        for node in nodes:
            line = node.line
            lines.append(indent + f"// Fortran line {line}")
            expression = lambda text, constant=False: self.expr(text, state, line, constant)
            if node.kind == "decl":
                self.require(not executable, line, "declarations must precede executable statements at program scope")
                typ, parameter, items = node.data
                for name, initializer in items:
                    self.require(name not in self.symbols, line, f"duplicate declaration `{name}`")
                    self.require(not parameter or initializer is not None, line, "PARAMETER requires an initializer")
                    value = self.cast(expression(initializer, parameter), typ, line) if initializer is not None else None
                    self.symbols[name] = typ
                    if parameter:
                        self.parameters.add(name)
                    binding = "let " if parameter else "let mut "
                    lines.append(indent + binding + self.rust_name(name) + ": " + typ + (" = " + value.code if value else "") + ";")
                    if value:
                        state.add(name)
                continue
            executable = True
            if node.kind == "assign":
                name, text = node.data
                typ = self.variable(name, line)
                self.require(name not in self.parameters, line, f"cannot assign PARAMETER `{name}`")
                self.require(name not in active_loops, line, f"cannot modify active DO variable `{name}`")
                value = self.cast(expression(text), typ, line)
                lines.append(indent + f"{self.rust_name(name)} = {value.code};")
                state.add(name)
            elif node.kind == "print":
                values = []
                for item in node.data:
                    if isinstance(item, ast.Constant) and isinstance(item.value, str):
                        values.append(rust_string(item.value) + ".to_string()")
                    else:
                        value = expression(item)
                        values.append(f'(if {value.code} {{ "T" }} else {{ "F" }}).to_string()' if value.typ == "bool" else f"({value.code}).to_string()")
                code = 'println!(" {}", [' + ", ".join(values) + '].join(" "));' if values else 'println!();'
                lines.append(indent + code)
            elif node.kind == "if":
                condition = expression(node.data)
                self.require(condition.typ == "bool", line, "IF condition must be LOGICAL")
                yes, yes_state = self.block(node.body, state, level=level+1, active_loops=active_loops)
                no, no_state = self.block(node.otherwise, state, level=level+1, active_loops=active_loops)
                lines += [indent + f"if {condition.code} {{", *yes, indent + "} else {", *no, indent + "}"]
                state = yes_state & no_state
            elif node.kind == "do":
                name, bounds = node.data
                self.require(self.variable(name, line) == "i32", line, "DO variable must be default INTEGER")
                self.require(name not in self.parameters and name not in active_loops, line, "invalid DO variable")
                values = [expression(text) for text in bounds]
                self.require(all(value.typ == "i32" for value in values), line, "DO bounds and stride must be default INTEGER")
                if len(values) == 2:
                    values.append(Value("1i32", "i32"))
                self.loop_id += 1
                prefix = f"rf_do_{self.loop_id}"
                start, limit, step = (prefix + suffix for suffix in ("_start", "_limit", "_step"))
                var = self.rust_name(name)
                body, _ = self.block(node.body, state | {name}, level=level+2, active_loops=active_loops | {name})
                lines += [indent + "{", *[indent + f"    let {target} = {value.code};" for target, value in zip((start, limit, step), values)],
                          indent + f'    assert_ne!({step}, 0, "zero DO stride at Fortran line {line}");',
                          indent + f"    {var} = {start};",
                          indent + f"    while if {step} > 0 {{ {var} <= {limit} }} else {{ {var} >= {limit} }} {{",
                          *body, indent + f'        {var} = {var}.checked_add({step}).expect("DO variable overflow");',
                          indent + "    }", indent + "}"]
                state.add(name)
            elif node.kind != "noop":
                raise Unsupported(line, "unsupported internal statement")
        return lines, state


RUNTIME = '''// Generated by rust-transpiler/transpile.py; no Fortran runtime or FFI.
#![allow(unused_mut, unused_variables, unused_assignments, dead_code, unused_parens)]

fn rf_to_i32(value: f64) -> i32 {
    let value = value.trunc();
    assert!(value.is_finite() && value >= i32::MIN as f64 && value <= i32::MAX as f64,
            "numeric conversion is outside default INTEGER range");
    value as i32
}

fn rf_ipow(base: i32, exponent: i32) -> i32 {
    if exponent < 0 {
        assert_ne!(base, 0, "zero to a negative power");
        match base {
            1 => 1,
            -1 => if exponent % 2 == 0 { 1 } else { -1 },
            _ => 0,
        }
    } else {
        base.checked_pow(exponent as u32).expect("INTEGER power overflow")
    }
}
'''


def transpile(source, *, compiler=None, filename="input.f90"):
    def emit(tree):
        body, _ = Emitter().block(tree, set(), declarations=True)
        return RUNTIME + "\nfn main() {\n" + "\n".join(body) + "\n}\n"
    return Frontend(source, filename=filename, compiler=compiler).process(emit)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("-o", "--out", type=Path, required=True)
    parser.add_argument("--rfort", type=Path, help="Existing compiler executable; no compiler build is performed")
    parser.add_argument("--run", action="store_true", help="Compile generated Rust with rustc and run it")
    args = parser.parse_args()
    try:
        generated = transpile(args.source.read_text(encoding="utf-8-sig"), compiler=args.rfort, filename=args.source)
        if args.out.resolve() == args.source.resolve():
            parser.error("output must differ from the input source")
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(generated, encoding="utf-8")
        if args.run:
            executable = args.out.with_suffix(".exe" if sys.platform == "win32" else ".bin").resolve()
            subprocess.run(["rustc", "--edition=2021", "-D", "warnings", str(args.out), "-o", str(executable)], check=True, timeout=120)
            return subprocess.run([str(executable)], timeout=10).returncode
    except Unsupported as error:
        print(f"{args.source}:{error.line}: error: {error}", file=sys.stderr)
        return 1
    except FrontendError as error:
        print(str(error), file=sys.stderr)
        return 1
    except (OSError, subprocess.SubprocessError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

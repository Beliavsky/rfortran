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
from rust_style import Plan, fields_fit, integer_constant, integer_range, simple_fields


@dataclass
class Value:
    code: str
    typ: str


NUMERIC = {"i32", "f32", "f64"}
FLOAT = {"f32", "f64"}


def rust_string(text):
    return '"' + "".join('\\"' if c == '"' else "\\\\" if c == "\\" else f"\\u{{{ord(c):x}}}" if ord(c) < 32 or ord(c) == 127 else c for c in text) + '"'


class Emitter:
    def __init__(self, plan=None, *, source_comments=False):
        self.symbols = {}
        self.parameters = set()
        self.loop_id = 0
        self.needs_format_runtime = False
        self.source_comments = source_comments
        self.plan = plan or Plan([])
        self.ranges = dict(self.plan.constants)
        self.identifiers = {}
        self.used_identifiers = set()

    def rust_name(self, name):
        return self.identifiers[name]

    def identifier(self, name, constant=False):
        keywords = set("as async await break const continue crate dyn else enum extern false fn for gen if impl in let loop match mod move mut pub ref return self static struct super trait true type unsafe use where while abstract become box do final macro override priv typeof unsized virtual yield try union".split())
        candidate = name.upper() if constant else name
        if candidate in keywords or candidate.startswith("rf_"):
            candidate = "v_" + name
        base, index = candidate, 1
        while candidate in self.used_identifiers:
            candidate = f"{base}_{index}"
            index += 1
        self.used_identifiers.add(candidate)
        self.identifiers[name] = candidate
        return candidate

    def readable(self, node, validated):
        """Prefer ordinary Rust expressions only with a proof of INTEGER safety."""
        def render(item):
            if isinstance(item, ast.Constant) and type(item.value) is int:
                return str(item.value)
            if isinstance(item, ast.Name):
                return self.rust_name(item.id.removeprefix("_rf_name_"))
            if isinstance(item, ast.BinOp) and integer_range(item,self.ranges) is not None:
                left, right = render(item.left), render(item.right)
                if left is None or right is None:
                    return None
                if isinstance(item.op, ast.Pow):
                    exponent = integer_constant(item.right,self.ranges)
                    if exponent == 2 and isinstance(item.left,(ast.Name,ast.Constant)):
                        return f"({left} * {left})"
                    if exponent is not None and exponent >= 0:
                        receiver = left + "i32" if isinstance(item.left,ast.Constant) else left
                        return f"({receiver}).pow({exponent})"
                    return None
                operator = {ast.Add:"+",ast.Sub:"-",ast.Mult:"*",ast.Div:"/"}.get(type(item.op))
                return f"({left} {operator} {right})" if operator else None
            if isinstance(item, ast.Call) and isinstance(item.func,ast.Name) and len(item.args) == 1:
                name = item.func.id.removeprefix("_rf_name_")
                arg = render(item.args[0])
                if arg is not None and name in ("real","dble"):
                    return f"({arg} as {'f32' if name == 'real' else 'f64'})"
                if arg is not None and name == "sqrt":
                    return f"{arg}.sqrt()" if arg.startswith("(") else f"({arg}).sqrt()"
            return None
        return render(node) or validated.code

    def direct_format(self, fmt, items, initialized, line):
        fields = simple_fields(fmt.strip(),len(items))
        if fields is None:
            return None
        values = [self.expr(item,initialized,line) for item in items]
        if not fields_fit(fields,items,values,self.ranges):
            return None
        template, arguments = [], []
        for (_,width,precision),item,value in zip(fields,items,values):
            spec = str(width) if width else ""
            if precision is not None:
                spec += f".{precision}"
            name = self.rust_name(item.id.removeprefix("_rf_name_")) if isinstance(item,ast.Name) else ""
            template.append("{" + name + (":"+spec if spec else "") + "}")
            if not name:
                arguments.append(self.readable(item,value))
        return "println!(" + rust_string("".join(template)) + (", " + ", ".join(arguments) if arguments else "") + ");"

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

    def block(self, nodes, initialized, *, level=1, active_loops=frozenset(), declarations=False, ranges=None):
        previous_ranges = self.ranges
        self.ranges = dict(previous_ranges if ranges is None else ranges)
        lines, state = [], set(initialized)
        indent = "    " * level
        executable = not declarations
        for node in nodes:
            line = node.line
            if self.source_comments:
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
                    known = integer_constant(initializer,self.ranges) if parameter and typ == "i32" and initializer is not None else None
                    literal_const = parameter and initializer is not None and (isinstance(initializer,ast.Constant) and type(initializer.value) is bool or isinstance(initializer,ast.Call) and isinstance(initializer.func,ast.Name) and initializer.func.id in ("_rf_f32","_rf_f64") and typ in FLOAT)
                    is_const = known is not None or literal_const
                    self.identifier(name,is_const)
                    binding = "const " if is_const else "let " if parameter else "let mut "
                    code = str(known) if known is not None else value.code if value else None
                    if not (initializer is None and name in self.plan.scoped_controls):
                        lines.append(indent + binding + self.rust_name(name) + ": " + typ + (" = " + code if code is not None else "") + ";")
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
            elif node.kind == "formatted_print":
                fmt, items = node.data
                direct = self.direct_format(fmt,items,state,line)
                if direct is not None:
                    lines.append(indent + direct)
                    continue
                self.needs_format_runtime = True
                values = []
                for item in items:
                    if isinstance(item, ast.Constant) and isinstance(item.value, str):
                        values.append(f"rf_format::Value::Character({rust_string(item.value)}.as_bytes())")
                        continue
                    value = expression(item)
                    constructor = {"i32":"Integer", "f32":"Real", "f64":"Real", "bool":"Logical"}[value.typ]
                    if value.typ == "i32":
                        arguments = f"({value.code}) as i64, 4"
                    elif value.typ in FLOAT:
                        arguments = f"({value.code}) as f64, {4 if value.typ == 'f32' else 8}"
                    else:
                        arguments = value.code
                    values.append(f"rf_format::Value::{constructor}({arguments})")
                lines.append(indent + f"rf_formatted_print({rust_string(fmt)}, &[{', '.join(values)}], {line});")
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
                child_ranges = dict(self.ranges)
                lower = integer_range(bounds[0],self.ranges)
                upper = integer_range(bounds[1],self.ranges)
                if lower and upper:
                    child_ranges[name] = (min(lower[0],upper[0]),max(lower[1],upper[1]))
                else:
                    child_ranges.pop(name,None)
                if id(node) in self.plan.range_loops:
                    body, _ = self.block(node.body,state | {name},level=level+1,active_loops=active_loops | {name},ranges=child_ranges)
                    start_code, end_code = (self.readable(bound,value) for bound,value in zip(bounds[:2],values[:2]))
                    if not any(isinstance(part,ast.Name) and part.id.removeprefix("_rf_name_") in self.symbols for bound in bounds[:2] for part in ast.walk(bound)):
                        # A literal-only range otherwise leaves method calls on
                        # its Rust control variable with an ambiguous integer type.
                        start_code = values[0].code
                    lines += [indent + f"for {var} in {start_code}..={end_code} {{", *body, indent + "}"]
                    state.add(name)
                    continue
                body, _ = self.block(node.body, state | {name}, level=level+2, active_loops=active_loops | {name},ranges=child_ranges)
                lines += [indent + "{", *[indent + f"    let {target} = {value.code};" for target, value in zip((start, limit, step), values)],
                          indent + f'    assert_ne!({step}, 0, "zero DO stride at Fortran line {line}");',
                          indent + f"    {var} = {start};",
                          indent + f"    while if {step} > 0 {{ {var} <= {limit} }} else {{ {var} >= {limit} }} {{",
                          *body, indent + f'        {var} = {var}.checked_add({step}).expect("DO variable overflow");',
                          indent + "    }", indent + "}"]
                state.add(name)
            elif node.kind != "noop":
                raise Unsupported(line, "unsupported internal statement")
        self.ranges = previous_ranges
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


def transpile(source, *, compiler=None, filename="input.f90", source_comments=False):
    def emit(tree):
        emitter = Emitter(Plan(tree), source_comments=source_comments)
        body, _ = emitter.block(tree, set(), declarations=True)
        shared = format_runtime() if emitter.needs_format_runtime else ""
        code = "\n".join(body)
        conversion = RUNTIME.index("fn rf_to_i32")
        power = RUNTIME.index("fn rf_ipow")
        runtime = RUNTIME[:conversion]
        if "rf_to_i32(" in code:
            runtime += RUNTIME[conversion:power]
        if "rf_ipow(" in code:
            runtime += RUNTIME[power:]
        return runtime + shared + "\nfn main() {\n" + code + "\n}\n"
    return Frontend(source, filename=filename, compiler=compiler).process(emit)


def format_runtime():
    root = Path(__file__).resolve().parent.parent
    # Embed the existing runtime verbatim: no second format parser/renderer and
    # no generated-code dependency on the source checkout or a Fortran runtime.
    source = (root / "src" / "format_spec.rs").read_text(encoding="utf-8")
    license_text = (root / "LICENSE-MIT").read_text(encoding="utf-8")
    notice = "\n// Shared formatting code from rfortran/src/format_spec.rs.\n"
    notice += "\n".join("// " + line if line else "//" for line in license_text.splitlines())
    return notice + "\nmod rf_format {\n" + source + "\n}\n" + FORMATTED_OUTPUT


FORMATTED_OUTPUT = '''
fn rf_formatted_print(format: &str, items: &[rf_format::Value<'_>], line: usize) {
    use std::io::Write;
    let mut output = Vec::new();
    rf_format::render_with(
        format.as_bytes(), items.len(),
        &|index, visit| visit(items[index]),
        &mut |byte| output.push(byte), true,
    ).unwrap_or_else(|error| panic!("Fortran line {line}: {error}"));
    output.push(b'\\n');
    std::io::stdout().lock().write_all(&output).expect("formatted output failed");
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("-o", "--out", type=Path,
                        help="Rust output path (default: target/rust-transpiler/<source-stem>.rs)")
    parser.add_argument("--rfort", type=Path, help="Existing compiler executable; no compiler build is performed")
    parser.add_argument("--run", action="store_true", help="Compile generated Rust with rustc and run it")
    parser.add_argument("--source-comments", action="store_true", help="Include original Fortran line comments in generated Rust")
    args = parser.parse_args()
    if args.out is None:
        args.out = Path("target") / "rust-transpiler" / (args.source.stem + ".rs")
    try:
        generated = transpile(args.source.read_text(encoding="utf-8-sig"), compiler=args.rfort, filename=args.source,
                              source_comments=args.source_comments)
        if args.out.resolve() == args.source.resolve():
            parser.error("output must differ from the input source")
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(generated, encoding="utf-8")
        if args.run:
            executable = args.out.with_suffix(".exe" if sys.platform == "win32" else ".bin").resolve()
            subprocess.run(["rustc", "--edition=2021", "--crate-name", "rfort_transpiled", "-D", "warnings", str(args.out), "-o", str(executable)], check=True, timeout=120)
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

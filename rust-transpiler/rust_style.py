"""Conservative Rust-oriented analysis over nodes already parsed by rfort."""
import ast
import math
import re
import struct

MIN, MAX = -(2**31), 2**31 - 1


def variable(node):
    return node.id.removeprefix("_rf_name_") if isinstance(node, ast.Name) else None


def names(expression):
    return {variable(item) for item in ast.walk(expression) if isinstance(item, ast.Name)}


def references(nodes):
    result = set()
    for node in nodes:
        if node.kind == "decl":
            for _, value in node.data[2]:
                if value is not None:
                    result |= names(value)
        elif node.kind == "assign":
            result.add(node.data[0])
            result |= names(node.data[1])
        elif node.kind == "array_decl":
            result |= names(node.data[2])
        elif node.kind == "assign_index":
            result.add(node.data[0])
            result |= names(node.data[1]) | names(node.data[2])
        elif node.kind=="assign_indices":
            result.add(node.data[0])
            for value in [*node.data[1],node.data[2]]:
                result |= names(value)
        elif node.kind in ("allocate","deallocate"):
            for value in node.data:
                result |= names(value)
        elif node.kind == "random_number":
            for value in node.data:
                result |= names(value)
        elif node.kind == "call":
            for value in node.data[1]:
                result |= names(value)
        elif node.kind == "return" and node.data is not None:
            result |= names(node.data)
        elif node.kind == "do":
            result.add(node.data[0])
            for value in node.data[1]:
                result |= names(value)
        elif node.kind == "if":
            result |= names(node.data)
        elif node.kind in ("print", "formatted_print"):
            for value in node.data if node.kind == "print" else node.data[1]:
                result |= names(value)
        result |= references(node.body) | references(node.otherwise)
    return result


def bounded(lo, hi):
    return (lo, hi) if MIN <= lo <= hi <= MAX else None


def integer_range(node, ranges):
    if isinstance(node, ast.Constant) and type(node.value) is int:
        return bounded(node.value, node.value)
    if isinstance(node, ast.Name):
        return ranges.get(variable(node))
    if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.USub):
        value = integer_range(node.operand, ranges)
        return bounded(-value[1], -value[0]) if value else None
    if not isinstance(node, ast.BinOp):
        return None
    left, right = integer_range(node.left,ranges), integer_range(node.right,ranges)
    if left is None or right is None:
        return None
    a,b = left
    c,d = right
    if isinstance(node.op, ast.Add):
        return bounded(a+c,b+d)
    if isinstance(node.op, ast.Sub):
        return bounded(a-d,b-c)
    if isinstance(node.op, ast.Mult):
        values = [x*y for x in left for y in right]
        return bounded(min(values),max(values))
    if isinstance(node.op, ast.Div) and not c <= 0 <= d:
        values = [(abs(x)//abs(y)) * (-1 if (x<0)!=(y<0) else 1) for x in left for y in right]
        return bounded(min(values),max(values))
    if isinstance(node.op, ast.Pow) and c == d and 0 <= c <= 31:
        values = [a**c,b**c]
        if a <= 0 <= b:
            values.append(0**c)
        return bounded(min(values),max(values))
    return None


def integer_constant(node, ranges):
    value = integer_range(node,ranges)
    return value[0] if value and value[0] == value[1] else None


def real_range(node, ranges):
    if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.USub):
        value = real_range(node.operand,ranges)
        return (-value[1],-value[0]) if value else None
    if not isinstance(node, ast.Call) or not isinstance(node.func, ast.Name) or len(node.args) != 1:
        return None
    function = node.func.id
    if function in ("_rf_f32","_rf_f64"):
        value = float(node.args[0].value)
        try:
            value = struct.unpack("f",struct.pack("f",value))[0] if function == "_rf_f32" else value
        except OverflowError:
            return None
        return (value,value) if math.isfinite(value) else None
    function = function.removeprefix("_rf_name_")
    if function in ("real","dble"):
        value = integer_range(node.args[0],ranges)
        if value:
            if function == "real":
                return tuple(struct.unpack("f",struct.pack("f",x))[0] for x in value)
            return tuple(float(x) for x in value)
    if function == "sqrt":
        value = real_range(node.args[0],ranges)
        if value and value[0] >= 0:
            # Deliberately loose, outward bounds: avoid relying on a host libm's
            # last bit to prove another platform's formatted output will fit.
            upper = 2*math.sqrt(value[1])+1
            return (0.0,upper) if math.isfinite(upper) else None
    return None


class Plan:
    def __init__(self, nodes):
        self.constants = {}
        for node in nodes:
            if node.kind == "decl" and node.data[:2] == ("i32",True):
                for name, value in node.data[2]:
                    known = integer_constant(value,self.constants) if value is not None else None
                    if known is not None:
                        self.constants[name] = (known,known)
        self.range_loops = set()
        self.analyze(nodes,set())
        controls = self.controls(nodes)
        self.scoped_controls = {name for name in controls if not self.outside(nodes,name)}

    def analyze(self, nodes, outer_live):
        for index,node in enumerate(nodes):
            live = references(nodes[index+1:]) | outer_live
            if node.kind == "do":
                name,bounds = node.data
                step = 1 if len(bounds) == 2 else integer_constant(bounds[2],self.constants)
                limit = integer_range(bounds[1],self.constants)
                if step == 1 and name not in live and limit and limit[1] < MAX:
                    self.range_loops.add(id(node))
                # Loop-carried observations require more analysis; keep nested
                # transformations conservative in their presence.
                self.analyze(node.body,live | references(node.body))
            else:
                self.analyze(node.body,live)
                self.analyze(node.otherwise,live)

    def controls(self,nodes):
        result = set()
        for node in nodes:
            if id(node) in self.range_loops:
                result.add(node.data[0])
            result |= self.controls(node.body) | self.controls(node.otherwise)
        return result

    def outside(self,nodes,name):
        for node in nodes:
            if node.kind == "do" and node.data[0] == name and id(node) in self.range_loops:
                if any(name in names(bound) for bound in node.data[1]):
                    return True
                continue
            if node.kind == "decl":
                if any(n == name and value is not None for n,value in node.data[2]):
                    return True
            if name in references([node]):
                return True
        return False


def simple_fields(format_string, count):
    """Recognize only flat Iw/Fw.d patterns; everything else uses format_spec.rs."""
    if not format_string.startswith("(") or not format_string.endswith(")"):
        return None
    result = []
    for field in format_string[1:-1].split(","):
        match = re.fullmatch(r"\s*(\d*)([iIfF])(\d+)(?:\.(\d+))?\s*",field)
        if not match:
            return None
        repeat,code,width,precision = match.groups()
        if (code.lower() == "f") != (precision is not None):
            return None
        repeat = int(repeat or "1")
        if repeat < 1 or repeat > count-len(result):
            return None
        result.extend([(code.lower(),int(width),int(precision) if precision is not None else None)]*repeat)
    return result if len(result) == count else None


def fields_fit(fields, items, values, ranges):
    for (code,width,precision),item,value in zip(fields,items,values):
        if code == "i":
            if value.typ != "i32":
                return False
            bounds = integer_range(item,ranges)
            if width and (bounds is None or max(len(str(x)) for x in bounds) > width):
                return False
        else:
            if value.typ not in ("f32","f64") or not width or not precision:
                return False
            bounds = real_range(item,ranges)
            if bounds is None:
                return False
            maximum = max(abs(x) for x in bounds)
            # Reserve a sign (also for negative zero), an integer digit, decimal
            # point, and carry from rounding. Narrow Fortran fields need fallback.
            digits = len(str(math.floor(maximum)+1))
            if width < 1+digits+1+precision:
                return False
    return True

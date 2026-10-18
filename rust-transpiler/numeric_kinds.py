"""Resolve numeric kinds over rfort's AST using the Rust backend's numeric model."""
from copy import deepcopy


class NumericKinds:
    # Match rfort's implemented binary32/binary64 model, rather than the Python
    # host's or a reference compiler's optional extended-precision kinds.
    ENV = dict(int8=1, int16=2, int32=4, int64=8, real32=4, real64=8,
               real128=-1, numeric_storage_size=32, character_storage_size=8,
               file_storage_size=8, input_unit=5, output_unit=6, error_unit=0,
               iostat_end=-1, iostat_eor=-2, iostat_inquire_internal_unit=-3)

    def __init__(self, line, unsupported):
        self.line = line
        self.unsupported = unsupported
        self.modules = {}

    def fail(self, line, message):
        raise self.unsupported(line, message)

    @staticmethod
    def selection(data, exported):
        renames = {remote.lower(): local.lower() for local, remote in data["renames"]}
        names = [name.lower() for name in data["only"]] if data["only"] is not None else list(exported)
        names += [name for name in renames if name not in names]
        return {renames.get(name, name): exported[name] for name in names if name in exported}

    def kind_number(self, text, constants, line):
        text = str(text).lower()
        try:
            return int(text)
        except ValueError:
            if text in constants:
                return constants[text]
        self.fail(line, f"numeric kind `{text}` requires a previously defined integer PARAMETER")

    def typ(self, value, constants, line):
        if not isinstance(value, dict):
            return value
        if "RealKind" in value:
            kind = self.kind_number(value["RealKind"], constants, line)
            if kind not in (4, 8):
                self.fail(line, f"unsupported REAL kind {kind}; Rust backend supports kinds 4 and 8")
            return "Real" if kind == 4 else "DoublePrecision"
        if "IntegerKind" in value or "Integer" in value:
            selector = value.get("IntegerKind", value.get("Integer"))
            kind = self.kind_number(4 if selector is None else selector, constants, line)
            if kind != 4:
                self.fail(line, f"unsupported INTEGER kind {kind}; Rust backend currently supports kind 4")
            return {"Integer": 4}
        return value

    def model(self, expr, constants, types):
        if "Ident" in expr:
            return types.get(expr["Ident"].lower())
        if "IntLit" in expr:
            return {"Integer": 4}
        if "RealLit" in expr:
            return "DoublePrecision" if "d" in expr["RealLit"].lower() else "Real"
        if "Logical" in expr:
            return "Logical"
        if "Un" in expr:
            return self.model(expr["Un"][1], constants, types)
        if "Bin" in expr:
            a = self.model(expr["Bin"][1], constants, types)
            b = self.model(expr["Bin"][2], constants, types)
            if a is None or b is None:
                return None
            return "DoublePrecision" if "DoublePrecision" in (a,b) else "Real" if "Real" in (a,b) else {"Integer":4}
        if "Call" in expr:
            name, args = expr["Call"]
            if name.lower() in ("real", "dble", "int") and len(args) == 1 and not ("Call" in args[0] and args[0]["Call"][0].startswith("@keyword:")):
                return {"real":"Real", "dble":"DoublePrecision", "int":{"Integer":4}}[name.lower()]
        return None

    def integer(self, expr, constants):
        if "IntLit" in expr:
            return int(expr["IntLit"])
        if "Ident" in expr:
            return constants.get(expr["Ident"].lower())
        if "Un" in expr and expr["Un"][0] == "Neg":
            value = self.integer(expr["Un"][1], constants)
            return -value if value is not None else None
        if "Bin" in expr:
            op, a, b = expr["Bin"]
            a, b = self.integer(a, constants), self.integer(b, constants)
            if a is None or b is None:
                return None
            if op == "Add": return a+b
            if op == "Sub": return a-b
            if op == "Mul": return a*b
            if op == "Div" and b: return (abs(a)//abs(b)) * (-1 if (a<0)!=(b<0) else 1)
            if op == "Pow" and 0 <= b <= 63: return a**b
        return None

    def arguments(self, name, args, keys, required, line):
        slots = [None] * len(keys)
        keyword = False
        for index, arg in enumerate(args):
            if "Call" in arg and arg["Call"][0].startswith("@keyword:"):
                keyword = True
                key = arg["Call"][0].split(":",1)[1].lower()
                if key not in keys:
                    self.fail(line, f"unknown keyword `{key}` in call to `{name}`")
                slot, arg = keys.index(key), arg["Call"][1][0]
            else:
                if keyword:
                    self.fail(line, f"positional argument after keyword argument in call to `{name}`")
                slot = index
            if slot >= len(keys) or slots[slot] is not None:
                self.fail(line, f"too many or duplicate arguments to `{name}`")
            slots[slot] = arg
        if any(slots[index] is None for index in required):
            self.fail(line, f"missing argument to `{name}`")
        return slots

    def expr(self, expr, constants, types, line):
        if not isinstance(expr, dict):
            return expr
        if "IntLit" in expr or "RealLit" in expr:
            tag = "IntLit" if "IntLit" in expr else "RealLit"
            text = expr[tag]
            if "_" in text:
                number, suffix = text.split("_",1)
                kind = self.kind_number(suffix, constants, line)
                if tag == "IntLit":
                    self.typ({"Integer":kind}, constants, line)
                    return {tag:number}
                self.typ({"RealKind":str(kind)}, constants, line)
                # The emitter uses the exponent letter to retain literal kind.
                number = number.lower().replace("d","e")
                if kind == 8:
                    number = number.replace("e","d") if "e" in number else number+"d0"
                return {tag:number}
            return expr
        if "Call" in expr:
            name, args = expr["Call"]
            name = name.lower()
            args = [self.expr(arg, constants, types, line) for arg in args]
            if name == "kind" and name not in types:
                arg, = self.arguments(name,args,["x"],[0],line)
                typ = self.model(arg, constants, types)
                if typ is not None:
                    return {"IntLit":str(8 if typ == "DoublePrecision" else 4)}
            if name in ("selected_int_kind", "selected_real_kind") and name not in types:
                keys = ["r"] if name == "selected_int_kind" else ["p","r","radix"]
                slots = self.arguments(name,args,keys,[0] if name == "selected_int_kind" else [],line)
                if not args:
                    self.fail(line, f"{name.upper()} requires an argument")
                values = [self.integer(arg, constants) if arg is not None else default
                          for arg, default in zip(slots,[0,0,2])]
                if None in values:
                    self.fail(line, f"{name.upper()} currently requires constant integer arguments")
                if name == "selected_int_kind":
                    r = values[0]
                    kind = next((k for limit,k in ((2,1),(4,2),(9,4),(18,8)) if r <= limit), -1)
                else:
                    p,r,radix = values
                    kind = (-5 if radix != 2 else 4 if p <= 6 and r <= 37 else
                            8 if p <= 15 and r <= 307 else -3 if p > 15 and r > 307 else
                            -1 if p > 15 else -2)
                return {"IntLit":str(kind)}
            return {"Call":[name,args]}
        return {key:self.walk(value, constants, types, line) for key,value in expr.items()}

    def walk(self, value, constants, types, line):
        if isinstance(value, dict):
            return self.expr(value, constants, types, line)
        if isinstance(value, list):
            result = []
            for item in value:
                if isinstance(item, dict) and "SourceLocation" in item:
                    line = self.line(item["SourceLocation"])
                result.append(self.walk(item, constants, types, line))
            return result
        return value

    def scope(self, body, inherited=None, line=1):
        constants, types = (dict(part) for part in (inherited or ({},{})))
        access, public = {}, True
        # A local declaration shadows a host-associated constant throughout scope.
        for item in body:
            if "Function" in item or "Subroutine" in item:
                data = item.get("Function", item.get("Subroutine"))
                types[data["name"].lower()] = None
            for tag in ("VarDecl", "ArrayDecl", "InitializedDecl"):
                if tag in item:
                    data = item[tag]
                    names = data["names"] if tag == "VarDecl" else [data["name"]] if tag == "ArrayDecl" else [n for n,_ in data["entities"]]
                    for name in names:
                        constants.pop(name.lower(), None)
                        types.pop(name.lower(), None)
        result = []
        for item in body:
            if "SourceLocation" in item:
                line = self.line(item["SourceLocation"])
            if "Module" in item:
                data = item["Module"]
                data["body"], exported = self.scope(data["body"], line=line)
                self.modules[data["name"].lower()] = exported
            elif "Function" in item or "Subroutine" in item:
                data = item.get("Function", item.get("Subroutine"))
                data["body"], _ = self.scope(data["body"], (constants,types), line)
                if "return_type" in data:
                    # A result may use a procedure-local kind parameter.
                    result_name = next((s["Return"]["Ident"].lower() for s in reversed(data["body"]) if isinstance(s.get("Return"),dict) and "Ident" in s["Return"]), data["name"].lower())
                    declared = next((s[tag]["kind"] for s in data["body"] for tag in ("VarDecl","InitializedDecl") if tag in s and result_name in (s[tag].get("names") or [n for n,_ in s[tag].get("entities",[])])), None)
                    data["return_type"] = declared if declared is not None else self.typ(data["return_type"],constants,line)
            elif "Use" in item:
                data = item["Use"]
                intrinsic = (data["module"].lower() == "iso_fortran_env"
                             and (str(data.get("nature","")).lower() == "intrinsic"
                                  or str(data.get("nature","")).lower() not in ("non_intrinsic","nonintrinsic")
                                  and data["module"].lower() not in self.modules))
                exported = ({name:(value,{"Integer":4}) for name,value in self.ENV.items()} if intrinsic else self.modules.get(data["module"].lower(),{}))
                selected = self.selection(data, exported)
                for name,(number,typ) in selected.items():
                    if number is not None: constants[name] = number
                    types[name] = typ
                if intrinsic:
                    wanted = [n.lower() for n in data["only"]] if data["only"] is not None else list(self.ENV)
                    wanted += [remote.lower() for _,remote in data["renames"]]
                    if any(name not in self.ENV for name in wanted):
                        self.fail(line,"unsupported ISO_FORTRAN_ENV entity in Rust backend")
                    item = {"InitializedDecl":{"kind":{"Integer":4},"attributes":["parameter"],"entities":[[name,{"IntLit":str(number)}] for name,(number,_) in selected.items()]}}
            elif "Accessibility" in item:
                data = item["Accessibility"]
                if data["names"]: access.update((n.lower(),data["public"]) for n in data["names"])
                else: public = data["public"]
            elif any(tag in item for tag in ("VarDecl","ArrayDecl","InitializedDecl")):
                tag = next(iter(item))
                data = item[tag]
                data["kind"] = self.typ(data["kind"],constants,line)
                names = data["names"] if tag == "VarDecl" else [data["name"]] if tag == "ArrayDecl" else [n for n,_ in data["entities"]]
                types.update((n.lower(),data["kind"]) for n in names)
                if tag == "InitializedDecl":
                    attrs = [a.lower() for a in data["attributes"]]
                    for index,(name,value) in enumerate(data["entities"]):
                        if "public" in attrs or "private" in attrs:
                            access[name.lower()] = "public" in attrs
                        if value is not None:
                            value = self.expr(value,constants,types,line)
                            data["entities"][index] = [name,value]
                            number = self.integer(value,constants)
                            if "parameter" in attrs and number is not None:
                                constants[name.lower()] = number
                elif tag == "ArrayDecl":
                    data["dims"] = self.walk(data["dims"],constants,types,line)
            else:
                item = self.walk(item,constants,types,line)
            result.append(item)
        exported = {name:(constants.get(name),typ) for name,typ in types.items() if access.get(name,public)}
        return result, exported

    def normalize(self, program):
        program = deepcopy(program)
        program["body"], _ = self.scope(program["body"])
        return program

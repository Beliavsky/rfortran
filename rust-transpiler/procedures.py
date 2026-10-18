"""Module scopes and scalar procedure interfaces over the compiler's AST."""
import ast
from compiler_frontend import Unsupported
from rust_style import Plan
from values import Value


class Modules:
    def __init__(self, nodes, emitter_class, source_comments=False, source_location=None):
        self.modules = {}
        self.emitters = []
        self.code = []
        self.source_location = source_location
        self.ready = set()
        allocator = emitter_class()
        for node in nodes:
            name = node.data["name"]
            allocator.require(name not in self.modules,node.line,f"duplicate module `{name}`")
            emitter = emitter_class(Plan(node.body),source_comments=source_comments,source_location=source_location)
            emitter.modules = self
            module = dict(node=node,emitter=emitter,rust=allocator.identifier(name),members={},exports={})
            self.modules[name] = module
            for item in node.body:
                if item.kind=="procedure":
                    member = dict(item.data,kind="procedure",node=item)
                    member_name = item.data["name"]
                    self.add_member(module,member_name,member,item.line)
                    emitter.identifier(member_name)
                elif item.kind=="decl":
                    typ,parameter,items = item.data
                    emitter.require(parameter,item.line,"mutable module variables are not yet supported")
                    for member_name,_ in items:
                        self.add_member(module,member_name,dict(kind="constant",typ=typ),item.line)
                        emitter.identifier(member_name,True)
                elif item.kind!="use":
                    raise Unsupported(item.line,f"unsupported module member: {item.kind}")
            for member_name,member in module["members"].items():
                member["path"] = f"crate::{module['rust']}::{emitter.rust_name(member_name)}"
        # USE dependencies must precede their consumers, as in the native frontend.
        for module in self.modules.values():
            node,emitter = module["node"],module["emitter"]
            scope = dict(module["members"])
            for item in node.body:
                if item.kind=="use":
                    self.merge(scope,self.imports(item),item.line)
            module["scope"] = scope
            module["exports"] = {name:member for name,member in scope.items()
                                 if node.data["access"].get(name,node.data["public"])}
            emitter.bind_members({name:member for name,member in scope.items() if name not in module["members"]},set())
            emitter.procedures.update({name:member for name,member in scope.items() if member["kind"]=="procedure"})
            declarations = [item for item in node.body if item.kind=="decl"]
            lines,_ = emitter.block(declarations,set(emitter.parameters),declarations=True)
            for member_name,member in module["members"].items():
                if member["kind"]=="constant":
                    member["range"] = emitter.plan.constants.get(member_name)
            if any(not line.lstrip().startswith(("const ","//")) for line in lines):
                raise Unsupported(node.line,"module PARAMETER initializers currently require supported Rust constant expressions")
            lines = [line.replace("const ","pub(crate) const ",1) if line.lstrip().startswith("const ") else line for line in lines]
            self.ready.add(node.data["name"])
            functions = []
            for item in node.body:
                if item.kind=="procedure":
                    functions.append(self.emit_procedure(module,item,emitter_class,source_comments))
            self.code.append("mod " + module["rust"] + " {\n    #[allow(unused_imports)]\n    use super::*;\n" + "\n".join(lines+functions) + "\n}\n")
            self.emitters.append(emitter)

    @staticmethod
    def add_member(module,name,member,line):
        if name in module["members"]:
            raise Unsupported(line,f"duplicate module member `{name}`")
        module["members"][name] = member

    @staticmethod
    def merge(scope,members,line):
        for name,member in members.items():
            if name in scope and scope[name]["path"] != member["path"]:
                raise Unsupported(line,f"ambiguous or conflicting USE-associated name `{name}`")
            scope[name] = member

    def imports(self,node):
        data = node.data
        name = data["module"].lower()
        if str(data.get("nature","")).lower()=="intrinsic":
            raise Unsupported(node.line,"intrinsic module interfaces are not yet supported")
        if name not in self.modules:
            raise Unsupported(node.line,f"unsupported USE: module `{name}` must be supplied in the translation group")
        if name not in self.ready:
            raise Unsupported(node.line,f"module `{name}` must precede the source that USEs it")
        exported = self.modules[name]["exports"]
        renames = {remote.lower():local.lower() for local,remote in data["renames"]}
        selected = [n.lower() for n in data["only"]] if data["only"] is not None else list(exported)
        selected += [remote for remote in renames if remote not in selected]
        result = {}
        for remote in selected:
            if remote not in exported:
                raise Unsupported(node.line,f"module `{name}` has no accessible member `{remote}`")
            local = renames.get(remote,remote)
            self.merge(result,{local:exported[remote]},node.line)
        return result

    def emit_procedure(self,module,node,emitter_class,source_comments):
        emitter = emitter_class(Plan(node.body),source_comments=source_comments,source_location=self.source_location)
        emitter.modules,emitter.procedure = self,node.data
        local = {name for name,_,_ in node.data["params"]}
        for item in node.body:
            if item.kind=="decl":
                local.update(name for name,_ in item.data[2])
            elif item.kind=="array_decl":
                local.add(item.data[1])
        scope = {name:member for name,member in module["scope"].items() if name not in local}
        state = set()
        emitter.bind_members(scope,state)
        arguments,prefix = [],[]
        for name,typ,intent in node.data["params"]:
            emitter.symbols[name] = typ
            rust = emitter.identifier(name)
            if intent=="out":
                prefix.append(f"        let mut {rust}: {typ};")
            else:
                arguments.append(f"{'mut ' if intent=='inout' else ''}{rust}: {typ}")
                state.add(name)
                if intent=="in":
                    emitter.readonly.add(name)
        body,state = emitter.block(node.body,state,level=2,declarations=True)
        if node.data["result"] is None:
            if not node.body or node.body[-1].kind!="return":
                body.append("        "+emitter.procedure_return(None,state,node.line))
        elif not node.body or node.body[-1].kind!="return":
            # The parser does not always emit a synthetic final RETURN. Explicit
            # RETURN nodes identify RESULT storage; otherwise a function without
            # RESULT stores its value in the local bearing the function's name.
            def result_expression(nodes):
                for item in nodes:
                    if item.kind=="return" and item.data is not None:
                        return item.data
                    value = result_expression(item.body+item.otherwise)
                    if value is not None:
                        return value
                return None
            result = result_expression(node.body)
            if result is None and node.data["name"] in emitter.symbols:
                result = ast.Name(id="_rf_name_"+node.data["name"])
            emitter.require(result is not None,node.line,"function AST requires a result expression")
            body.append("        "+emitter.procedure_return(result,state,node.line))
        emitter.require(not emitter.needs_random_runtime,node.line,"RANDOM_NUMBER inside procedures is not yet supported")
        typ = node.data["result"] or output_type(node.data)
        signature = f"    #[allow(unreachable_code)]\n    pub(crate) fn {module['emitter'].rust_name(node.data['name'])}({', '.join(arguments)}) -> {typ} {{\n"
        self.emitters.append(emitter)
        return signature+"\n".join(prefix+body)+"\n    }"


def output_type(procedure):
    types = [typ for _,typ,intent in procedure["params"] if intent!="in"]
    return types[0] if len(types)==1 else "("+", ".join(types)+")"


class ProcedureSupport:
    def bind_members(self,members,state,line=1):
        for name,member in members.items():
            if member["kind"]=="procedure":
                if name in self.symbols or name in self.procedures and self.procedures[name]["path"]!=member["path"]:
                    raise Unsupported(line,f"conflicting USE-associated name `{name}`")
                self.procedures[name] = member
            else:
                if name in self.parameters and self.identifiers.get(name)==member["path"]:
                    state.add(name)
                    continue
                self.require(name not in self.symbols and name not in self.procedures,line,f"conflicting USE-associated name `{name}`")
                self.symbols[name] = member["typ"]
                self.identifiers[name] = member["path"]
                self.parameters.add(name)
                state.add(name)
                if member.get("range") is not None:
                    self.plan.constants[name] = member["range"]
                    self.ranges[name] = member["range"]

    def function_call(self,name,args,state,line,constant):
        self.require(not constant,line,"procedure calls are not supported in PARAMETER initializers")
        proc = self.procedures[name]
        self.require(proc["result"] is not None,line,f"`{name}` is a subroutine, not a function")
        self.require(len(args)==len(proc["params"]),line,f"wrong number of arguments to `{name}`")
        values = [self.expr(arg,state,line) for arg in args]
        for value,(_,typ,_) in zip(values,proc["params"]):
            self.require(value.extent is None and value.typ==typ,line,f"argument type/kind/rank mismatch in call to `{name}`")
        return Value(f"{proc['path']}({', '.join(v.code for v in values)})",proc["result"])

    def subroutine_call(self,name,args,state,line,active_loops):
        self.require(name in self.procedures,line,f"unsupported Rust backend call: {name}")
        proc = self.procedures[name]
        self.require(proc["result"] is None,line,f"`{name}` is a function, not a subroutine")
        self.require(len(args)==len(proc["params"]),line,f"wrong number of arguments to `{name}`")
        inputs,outputs = [],[]
        for arg,(_,typ,intent) in zip(args,proc["params"]):
            if intent!="in":
                self.require(isinstance(arg,ast.Name),line,"output arguments currently require scalar variables")
                actual = arg.id.removeprefix("_rf_name_")
                self.require(actual not in self.arrays and self.variable(actual,line)==typ,line,f"argument type/kind/rank mismatch in call to `{name}`")
                self.require(actual not in self.parameters and actual not in self.readonly and actual not in active_loops,line,f"cannot modify argument `{actual}`")
                self.require(actual not in outputs,line,"aliased output arguments are not supported")
                outputs.append(actual)
            if intent!="out":
                value = self.expr(arg,state,line)
                self.require(value.typ==typ and value.extent is None,line,f"argument type/kind/rank mismatch in call to `{name}`")
                inputs.append(value.code)
        code = f"{proc['path']}({', '.join(inputs)})"
        if not outputs:
            return [code+";"]
        lines = ["let rf_call_result = "+code+";"]
        for index,name in enumerate(outputs):
            value = "rf_call_result" + (f".{index}" if len(outputs)>1 else "")
            lines.append(f"{self.rust_name(name)} = {value};")
            state.add(name)
        return lines

    def procedure_return(self,expression,state,line):
        self.require(self.procedure is not None,line,"RETURN is only supported inside a procedure")
        typ = self.procedure["result"]
        if typ is not None:
            self.require(expression is not None,line,"function RETURN requires its result expression")
            value = self.expr(expression,state,line)
            self.require(value.typ==typ and value.extent is None,line,"function result type/kind/rank mismatch")
            return "return "+value.code+";"
        self.require(expression is None,line,"subroutine RETURN cannot have a value")
        outputs = []
        for name,_,intent in self.procedure["params"]:
            if intent!="in":
                self.require(name in state,line,f"output argument `{name}` is not definitely assigned on return")
                outputs.append(self.rust_name(name))
        code = outputs[0] if len(outputs)==1 else "("+", ".join(outputs)+")"
        return "return "+code+";"

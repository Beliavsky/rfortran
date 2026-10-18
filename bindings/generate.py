"""Standalone, manifest-driven Fortran-to-Rust binding prototype (no compiler imports)."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys

TYPES = {
    "f32": "real(c_float)",
    "f64": "real(c_double)",
    "i32": "integer(c_int32_t)",
    "i64": "integer(c_int64_t)",
}
RUST_KEYWORDS = set("as async await break const continue crate dyn else enum extern false fn for gen if impl in let loop match mod move mut pub ref return self static struct super trait true type unsafe use where while abstract become box do final macro override priv typeof unsized virtual yield try union".split())


class SpecError(ValueError):
    pass


def object_fields(value, allowed, required, context):
    if not isinstance(value, dict):
        raise SpecError(f"{context}: expected an object")
    unknown = value.keys() - allowed
    missing = required - value.keys()
    if unknown:
        raise SpecError(f"{context}: unsupported fields: {', '.join(sorted(unknown))}")
    if missing:
        raise SpecError(f"{context}: missing fields: {', '.join(sorted(missing))}")


def identifier(value, context, *, rust=False, limit=63):
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", value) or len(value) > limit:
        raise SpecError(f"{context}: expected an ASCII identifier of at most {limit} characters")
    value = value.lower()
    if value.startswith("bg_") or (rust and value in RUST_KEYWORDS):
        raise SpecError(f"{context}: reserved identifier {value!r}; choose another public/argument name")
    return value


def type_name(value, context):
    if not isinstance(value, str) or value not in TYPES:
        raise SpecError(f"{context}: supported types are {', '.join(TYPES)}")
    return value


def validate(spec):
    object_fields(spec, {"library", "procedures"}, {"library", "procedures"}, "manifest")
    library = identifier(spec["library"], "library", limit=24)
    procedures = spec["procedures"]
    if not isinstance(procedures, list) or not procedures:
        raise SpecError("procedures: expected a nonempty list")
    normalized, names = [], set()
    for index, procedure in enumerate(procedures):
        where = f"procedures[{index}]"
        object_fields(procedure, {"module", "name", "rust_name", "kind", "result", "args", "same_length"},
                      {"module", "name", "kind", "args"}, where)
        module = identifier(procedure["module"], where + ".module")
        name = identifier(procedure["name"], where + ".name")
        public = identifier(procedure.get("rust_name", name), where + ".rust_name", rust=True)
        if public in names:
            raise SpecError(f"{where}: duplicate Rust name {public!r}; specify a unique rust_name")
        names.add(public)
        kind = procedure["kind"]
        if kind not in ("function", "subroutine"):
            raise SpecError(f"{where}.kind: expected function or subroutine")
        result = None
        if kind == "function":
            result = type_name(procedure.get("result"), where + ".result")
        elif "result" in procedure:
            raise SpecError(f"{where}: subroutines cannot declare a result")
        if not isinstance(procedure["args"], list):
            raise SpecError(f"{where}.args: expected a list")
        args, argument_names = [], set()
        for arg in procedure["args"]:
            object_fields(arg, {"name", "type", "intent", "rank"}, {"name", "type", "intent"}, where + ".args")
            argname = identifier(arg["name"], where + ".argument name", rust=True)
            if argname in argument_names:
                raise SpecError(f"{where}: duplicate argument {argname!r}")
            argument_names.add(argname)
            intent = arg["intent"]
            if intent not in ("in", "out", "inout"):
                raise SpecError(f"{where}.{argname}: expected intent in, out, or inout")
            rank = arg.get("rank", 0)
            if type(rank) is not int or rank not in (0, 1):
                raise SpecError(f"{where}.{argname}: only scalar and rank-one arguments are supported")
            if rank and len(argname) > 58:
                raise SpecError(f"{where}.{argname}: array argument names are limited to 58 characters")
            args.append({"name": argname, "type": type_name(arg["type"], where + "." + argname),
                         "intent": intent, "rank": rank})
        groups = procedure.get("same_length", [])
        if not isinstance(groups, list):
            raise SpecError(f"{where}.same_length: expected a list of argument groups")
        array_names = {arg["name"] for arg in args if arg["rank"] == 1}
        checked_groups = []
        for group in groups:
            if not isinstance(group, list) or len(group) < 2:
                raise SpecError(f"{where}.same_length: each group needs at least two arrays")
            group = [identifier(item, where + ".same_length") for item in group]
            if len(set(group)) != len(group) or not set(group) <= array_names:
                raise SpecError(f"{where}.same_length: use distinct rank-one argument names")
            checked_groups.append(group)
        normalized.append({"module": module, "name": name, "rust_name": public,
                           "kind": kind, "result": result, "args": args, "same_length": checked_groups})
    return {"library": library, "procedures": normalized}


def symbol(spec, procedure):
    return f"rbg_{spec['library']}_{procedure['rust_name']}"


def argument_list(args, prefix, suffix, indent="      "):
    # One argument per continued line keeps even long identifiers within 132 columns.
    if not args:
        return [prefix + suffix]
    return [prefix + " &", *[indent + arg + (", &" if i < len(args)-1 else suffix)
                              for i, arg in enumerate(args)]]


def fortran_source(spec):
    lines = ["! Generated by bindings/generate.py; do not edit.",
             f"module rbg_{spec['library']}", "   implicit none", "   private", "contains"]
    for index, proc in enumerate(spec["procedures"]):
        wrapper = f"bg_wrap_{index}"
        abi_args = []
        for arg in proc["args"]:
            if arg["rank"]:
                abi_args.append("bg_n_" + arg["name"])
            abi_args.append(arg["name"])
        function = proc["kind"] == "function"
        ending = ") result(bg_result) &" if function else ") &"
        lines += argument_list(abi_args, f"   {proc['kind']} {wrapper}(", ending)
        lines += [f'      bind(C, name="{symbol(spec, proc)}")',
                  "      use iso_c_binding, only: &",
                  "         bg_c_float => c_float, bg_c_double => c_double, &",
                  "         bg_c_int32_t => c_int32_t, bg_c_int64_t => c_int64_t, bg_c_size_t => c_size_t",
                  f"      use {proc['module']}, only: &", f"         bg_target => {proc['name']}", "      implicit none"]
        for arg in proc["args"]:
            name, rank, intent = arg["name"], arg["rank"], arg["intent"]
            if rank:
                lines.append(f"      integer(bg_c_size_t), value, intent(in) :: bg_n_{name}")
            value = ", value" if intent == "in" and not rank else ""
            extent = f"(bg_n_{name})" if rank else ""
            declaration = f"      {TYPES[arg['type']].replace('c_', 'bg_c_')}, intent({intent}){value} :: &"
            lines += [declaration, f"         {name}{extent}"]
        if function:
            lines.append(f"      {TYPES[proc['result']].replace('c_', 'bg_c_')} :: bg_result")
        call_args = [f"{arg['name']}={arg['name']}" for arg in proc["args"]]
        prefix = "      bg_result = bg_target(" if function else "      call bg_target("
        lines += argument_list(call_args, prefix, ")", indent="         ")
        lines += [f"   end {proc['kind']} {wrapper}", ""]
    lines.append(f"end module rbg_{spec['library']}")
    # Long public names are allowed, but generated Fortran helper names must also fit.
    if any(len(line) > 132 for line in lines):
        raise SpecError("Generated Fortran exceeds 132 columns; shorten the offending identifiers")
    return "\n".join(lines) + "\n"


def rust_source(spec):
    library = spec["library"]
    lines = ["// Generated by bindings/generate.py; do not edit.",
             "// Interface descriptions and native procedure preconditions require review.",
             "mod raw {", f'    #[cfg_attr(target_os = "windows", link(name = "{library}", kind = "raw-dylib"))]',
             f'    #[cfg_attr(not(target_os = "windows"), link(name = "{library}"))]', '    unsafe extern "C" {']
    for index, proc in enumerate(spec["procedures"]):
        parameters = []
        for arg in proc["args"]:
            name, typ = arg["name"], arg["type"]
            if arg["rank"]:
                parameters.append(f"bg_n_{name}: usize")
            if arg["rank"] or arg["intent"] != "in":
                typ = f"*{'const' if arg['intent'] == 'in' else 'mut'} {typ}"
            parameters.append(f"{name}: {typ}")
        result = f" -> {proc['result']}" if proc["result"] else ""
        lines += [f'        #[link_name = "{symbol(spec, proc)}"]',
                  f"        pub(super) fn bg_raw_{index}({', '.join(parameters)}){result};"]
    lines += ["    }", "}", ""]
    for index, proc in enumerate(spec["procedures"]):
        parameters, actual = [], []
        for arg in proc["args"]:
            name, typ, rank, intent = arg["name"], arg["type"], arg["rank"], arg["intent"]
            if rank:
                typ = f"&{'mut ' if intent != 'in' else ''}[{typ}]"
                actual.extend([f"{name}.len()", f"{name}.{'as_ptr' if intent == 'in' else 'as_mut_ptr'}()"])
            elif intent != "in":
                typ = f"&mut {typ}"
                actual.append(f"{name} as *mut _")
            else:
                actual.append(name)
            parameters.append(f"{name}: {typ}")
        result = f" -> {proc['result']}" if proc["result"] else ""
        lines += [f"/// Calls `{proc['module']}::{proc['name']}` through its generated Fortran wrapper.",
                  "///", "/// # Safety", "/// The manifest must match the linked Fortran procedure, including kinds and intents.",
                  "/// Arrays must match its assumed-shape interface and satisfy its extent/value requirements.",
                  "/// The procedure must not retain pointers, violate Rust aliasing, or access shared state",
                  "/// concurrently without synchronization. Honor all other native procedure preconditions.",
                  f"pub unsafe fn {proc['rust_name']}({', '.join(parameters)}){result} {{"]
        for arg in proc["args"]:
            if arg["rank"]:
                lines.append(f'    assert!({arg["name"]}.len() <= isize::MAX as usize, "array extent exceeds Fortran c_size_t range");')
        for group in proc["same_length"]:
            for name in group[1:]:
                lines.append(f'    assert_eq!({group[0]}.len(), {name}.len(), "{group[0]} and {name} must have equal lengths");')
        lines += [f"    unsafe {{ raw::bg_raw_{index}({', '.join(actual)}) }}", "}", ""]
    return "\n".join(lines)


def generate(spec):
    spec = validate(spec)
    return {"bindings.f90": fortran_source(spec), "bindings.rs": rust_source(spec)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--out-dir", required=True, type=Path)
    args = parser.parse_args()
    try:
        output = generate(json.loads(args.manifest.read_text(encoding="utf-8")))
        args.out_dir.mkdir(parents=True, exist_ok=True)
        for name, content in output.items():
            (args.out_dir / name).write_text(content, encoding="utf-8")
            print(args.out_dir / name)
    except (OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

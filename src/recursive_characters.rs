// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! INTENT(IN) CHARACTER scalar dummies of recursive procedures. A procedure
//! with a CHARACTER dummy is expanded at each call (crate::array_calls),
//! which recursion does not allow. So a procedure P that reaches itself and
//! has such dummies gets a clone, `rfort_rs_clone_P`, that takes an INTEGER
//! slot of a stack of strings in a generated module for each of them; on
//! entry the clone copies its slots into local buffers, and each dummy is
//! its buffer's first `len` characters. P keeps its interface, so that
//! direct and generic calls are unchanged: its body pushes its CHARACTER
//! dummies and calls the clone. Inside the clones, calls of converted
//! procedures push their actuals and call the clones. The slots of a
//! statement are popped after it (for IF and SELECT CASE, at the start of
//! each branch), once every call in it has copied its arguments.
//!
//! A string longer than BUFFER characters, more than DEPTH pending slots,
//! or more than POOL pending characters stop the program with an error.
//! Procedures with other CHARACTER dummies, ELEMENTAL procedures, functions
//! without a RESULT clause, and procedures whose internal procedures use
//! the dummies are left to the general lowering, as is every procedure when
//! a clone calls one where slots cannot be placed (DO and DO WHILE controls).
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use std::collections::{HashMap, HashSet};

pub(crate) const MODULE: &str = "rfort_recursive_strings";
/// The pending strings, one after another, with each slot's start and length.
const TEXT: &str = "rfort_rs_text";
const STARTS: &str = "rfort_rs_starts";
const LENGTHS: &str = "rfort_rs_lengths";
pub(crate) const TOP: &str = "rfort_rs_top";
pub(crate) const PUSH: &str = "rfort_rs_push";

/// Whether a name, perhaps qualified by its module, is the push subroutine
/// or the stack pointer, which PURE procedures may use (see crate::purity).
pub(crate) fn is_push(name: &str) -> bool {
    name == PUSH || name.ends_with(&format!("${PUSH}"))
}
pub(crate) fn is_top(name: &str) -> bool {
    name == TOP || name.ends_with(&format!("${TOP}"))
}
// Small enough that the interpreter, which copies strings, stays quick.
const BUFFER: usize = 256;
const DEPTH: usize = 1024;
/// A long scalar: CHARACTER arrays of this size are slow to lower.
const POOL: usize = 8192;

/// The converted dummies of a procedure: their positions and names.
#[derive(Clone)]
struct Converted {
    clone: String,
    /// Position, dummy name.
    dummies: Vec<(usize, String)>,
}

fn clone_name(procedure: &str) -> String {
    format!("rfort_rs_clone_{procedure}")
}
fn slot_name(dummy: &str) -> String {
    format!("rfort_rs_slot_{dummy}")
}
fn buffer_name(dummy: &str) -> String {
    format!("rfort_rs_buffer_{dummy}")
}
fn length_name(dummy: &str) -> String {
    format!("rfort_rs_length_{dummy}")
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}

pub(crate) fn resolve(mut p: Program) -> Program {
    // Procedures by name; a name defined more than once is ambiguous.
    let mut procedures: HashMap<String, (Vec<String>, Vec<Stmt>, bool)> = HashMap::new();
    let mut duplicate = HashSet::new();
    collect(&p.body, &mut procedures, &mut duplicate);
    for d in &duplicate {
        procedures.remove(d);
    }
    // The call graph, and the procedures that reach themselves.
    let edges: HashMap<&String, HashSet<String>> = procedures.iter().map(|(n, (_, body, _))| {
        let mut called = HashSet::new();
        calls(body, &mut called);
        (n, called)
    }).collect();
    let recursive: Vec<String> = procedures.keys().filter(|start| {
        let mut seen = HashSet::new();
        let mut stack: Vec<String> = edges[start].iter().cloned().collect();
        while let Some(n) = stack.pop() {
            if &&n == start { return true; }
            if seen.insert(n.clone()) {
                if let Some(next) = edges.get(&n) { stack.extend(next.iter().cloned()); }
            }
        }
        false
    }).cloned().collect();
    let mut converted: HashMap<String, Converted> = HashMap::new();
    for name in recursive {
        let (params, body, elemental) = &procedures[&name];
        if *elemental { continue; }
        let intent = |d: &str, i: &str| body.iter().any(|s| matches!(s, Stmt::DummyIntent { name, intent } if name == d && intent == i));
        let character = |d: &str| body.iter().any(|s| matches!(s,
            Stmt::VarDecl { kind: TypeSpec::Character(Some(_)) | TypeSpec::CharacterAssumed, names } if names.iter().any(|n| n == d)));
        let any_character = |d: &str| body.iter().any(|s| matches!(s,
            Stmt::VarDecl { kind: TypeSpec::Character(_) | TypeSpec::CharacterAssumed | TypeSpec::CharacterLength(_), names } if names.iter().any(|n| n == d))
            || matches!(s, Stmt::ArrayDecl { kind: TypeSpec::Character(_) | TypeSpec::CharacterAssumed | TypeSpec::CharacterLength(_), name, .. } if name == d));
        let mut dummies = vec![];
        let mut ok = true;
        for (i, d) in params.iter().enumerate() {
            if !any_character(d) { continue; }
            if character(d) && intent(d, "intent(in)") && !intent(d, "optional") {
                dummies.push((i, d.clone()));
            } else {
                ok = false;
            }
        }
        // Internal procedures would see the dummies by host association.
        let internal = body.iter().any(|s| matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. })
            && dummies.iter().any(|(_, d)| format!("{s:?}").contains(&format!("\"{d}\""))));
        // Specification expressions, such as `character(len=len(text))`,
        // would need the dummy before the clone copies it.
        let specification = body.iter().filter(|s| declaration(s)).any(|s| match s {
            Stmt::VarDecl { names, .. } if names.iter().all(|n| dummies.iter().any(|(_, d)| d == n)) => false,
            Stmt::DummyIntent { name, .. } if dummies.iter().any(|(_, d)| d == name) => false,
            s => dummies.iter().any(|(_, d)| format!("{s:?}").contains(&format!("\"{d}\""))),
        });
        // Recursive CHARACTER functions keep their diagnostic.
        let character_result = matches!(body.last(), Some(Stmt::Return(Some(Expr::Ident(r)))) if body.iter().any(|s| matches!(s,
            Stmt::VarDecl { kind: TypeSpec::Character(_) | TypeSpec::CharacterAssumed | TypeSpec::CharacterLength(_), names } if names.contains(r))));
        if ok && !dummies.is_empty() && !internal && !specification && !character_result {
            converted.insert(name.clone(), Converted { clone: clone_name(&name), dummies });
        }
    }
    // A function's clone needs a result variable other than its name.
    converted.retain(|name, _| {
        fn find<'a>(body: &'a [Stmt], name: &str) -> Option<&'a Stmt> {
            body.iter().find_map(|s| match s {
                Stmt::Function { name: n, .. } | Stmt::Subroutine { name: n, .. } if n == name => Some(s),
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => find(body, name),
                _ => None,
            })
        }
        match find(&p.body, name) {
            Some(Stmt::Function { body, .. }) => matches!(body.last(), Some(Stmt::Return(Some(Expr::Ident(r)))) if r != name),
            Some(_) => true,
            None => false,
        }
    });
    if converted.is_empty() {
        return p;
    }
    let original = p.clone();
    let mut failed = false;
    clones(&mut p.body, &converted, &mut failed);
    if failed {
        return original;
    }
    add_module(&mut p);
    p
}

fn collect(body: &[Stmt], out: &mut HashMap<String, (Vec<String>, Vec<Stmt>, bool)>, duplicate: &mut HashSet<String>) {
    for s in body {
        match s {
            Stmt::Function { name, params, body, elemental, .. } | Stmt::Subroutine { name, params, body, elemental, .. } => {
                if out.insert(name.clone(), (params.clone(), body.clone(), *elemental)).is_some() {
                    duplicate.insert(name.clone());
                }
                collect(body, out, duplicate);
            }
            Stmt::Module { body, .. } => collect(body, out, duplicate),
            _ => {}
        }
    }
}

fn calls(body: &[Stmt], out: &mut HashSet<String>) {
    for s in body {
        if let Stmt::CallSub { name, .. } = s { out.insert(name.clone()); }
        let mut s = s.clone();
        for e in crate::procedure_arguments::expressions(&mut s) {
            crate::procedure_arguments::visit_expr(e, &mut |e| {
                if let Expr::Call(n, _) = e { out.insert(n.clone()); }
            });
        }
        for child in crate::procedure_arguments::children(&mut s) {
            calls(child, out);
        }
    }
}

fn declaration(s: &Stmt) -> bool {
    matches!(s, Stmt::Use { .. } | Stmt::Implicit(_) | Stmt::VarDecl { .. } | Stmt::ArrayDecl { .. } | Stmt::InitializedDecl { .. }
        | Stmt::DerivedDecl { .. } | Stmt::DummyIntent { .. } | Stmt::SourceLocation(_) | Stmt::GenericInterface { .. }
        | Stmt::OperatorInterface { .. } | Stmt::DerivedType { .. } | Stmt::Accessibility { .. })
}

/// Give each converted procedure its clone, and make the procedure call it.
fn clones(body: &mut Vec<Stmt>, converted: &HashMap<String, Converted>, failed: &mut bool) {
    let mut out = vec![];
    for mut s in std::mem::take(body) {
        if let Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } = &mut s {
            clones(body, converted, failed);
        }
        let name = match &s {
            Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } => name.clone(),
            _ => {
                out.push(s);
                continue;
            }
        };
        let Some(c) = converted.get(&name) else {
            out.push(s);
            continue;
        };
        let mut clone = s.clone();
        clone_definition(&mut clone, c, converted, failed);
        wrapper(&mut s, c);
        out.push(s);
        out.push(clone);
    }
    *body = out;
}

/// The clone: slots for the dummies, copied into buffers on entry, and calls
/// of converted procedures through their clones.
fn clone_definition(s: &mut Stmt, c: &Converted, converted: &HashMap<String, Converted>, failed: &mut bool) {
    let (Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. }) = s else { return };
    *name = c.clone.clone();
    let dummies: Vec<String> = c.dummies.iter().map(|(_, d)| d.clone()).collect();
    for (i, d) in &c.dummies {
        params[*i] = slot_name(d);
    }
    body.retain(|s| !matches!(s, Stmt::VarDecl { names, .. } if names.len() == 1 && dummies.contains(&names[0])));
    for s in body.iter_mut() {
        match s {
            Stmt::VarDecl { names, .. } => names.retain(|n| !dummies.contains(n)),
            Stmt::DummyIntent { name, .. } if dummies.contains(name) => *name = slot_name(name),
            _ => {}
        }
    }
    let mut declarations = vec![];
    let mut entry = vec![];
    for d in &dummies {
        declarations.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![slot_name(d), length_name(d)] });
        declarations.push(Stmt::VarDecl { kind: TypeSpec::Character(Some(BUFFER)), names: vec![buffer_name(d)] });
        entry.push(Stmt::Assign { name: length_name(d), value: Expr::Call(LENGTHS.into(), vec![id(&slot_name(d))]) });
        let start = Expr::Call(STARTS.into(), vec![id(&slot_name(d))]);
        let end = bin(BinOp::Sub, bin(BinOp::Add, start.clone(), id(&length_name(d))), int(1));
        entry.push(Stmt::Assign { name: buffer_name(d), value: Expr::Section {
            name: TEXT.into(), lower: Some(Box::new(start)), upper: Some(Box::new(end)), stride: None } });
    }
    let mut rest: Vec<Stmt> = std::mem::take(body);
    for s in rest.iter_mut() {
        if !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
            references(s, &dummies);
        }
    }
    let at = rest.iter().position(|s| !declaration(s)).unwrap_or(rest.len());
    let mut tail = rest.split_off(at);
    let mut serial = 0;
    let mut slots = vec![];
    statements(&mut tail, converted, failed, &mut serial, &mut slots);
    if !slots.is_empty() {
        declarations.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: slots });
    }
    let uses = rest.iter().take_while(|s| matches!(s, Stmt::Use { .. } | Stmt::SourceLocation(_))).count();
    rest.splice(uses..uses, declarations);
    rest.extend(entry);
    rest.extend(tail);
    *body = rest;
}

/// The procedure itself pushes its CHARACTER dummies and calls its clone.
fn wrapper(s: &mut Stmt, c: &Converted) {
    let function = matches!(s, Stmt::Function { .. });
    let (Stmt::Function { params, body, .. } | Stmt::Subroutine { params, body, .. }) = s else { return };
    let result = match body.last() {
        Some(Stmt::Return(Some(Expr::Ident(r)))) => Some(r.clone()),
        _ => None,
    };
    let mut declarations: Vec<Stmt> = body.iter().take_while(|s| declaration(s)).cloned().collect();
    let mut executable = vec![];
    let mut actuals: Vec<Expr> = params.iter().map(|p| id(p)).collect();
    let mut slots = vec![];
    for (i, d) in &c.dummies {
        let slot = format!("rfort_rs_entry_{d}");
        executable.push(Stmt::CallSub { name: PUSH.into(), args: vec![id(d)] });
        executable.push(Stmt::Assign { name: slot.clone(), value: id(TOP) });
        actuals[*i] = id(&slot);
        slots.push(slot);
    }
    declarations.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: slots });
    let call = Expr::Call(c.clone.clone(), actuals.clone());
    match (function, &result) {
        (true, Some(r)) => executable.push(Stmt::Assign { name: r.clone(), value: call }),
        _ => executable.push(Stmt::CallSub { name: c.clone.clone(), args: actuals }),
    }
    executable.push(Stmt::Assign { name: TOP.into(), value: bin(BinOp::Sub, id(TOP), int(c.dummies.len() as i64)) });
    declarations.extend(executable);
    if let Some(r) = result {
        declarations.push(Stmt::Return(Some(id(&r))));
    }
    *body = declarations;
}

/// The dummy `d` is `buffer(1:length)`.
fn references(s: &mut Stmt, dummies: &[String]) {
    let rewrite = |e: &mut Expr| {
        crate::procedure_arguments::visit_expr(e, &mut |e| match e {
            Expr::Ident(n) if dummies.contains(n) => {
                *e = Expr::Section { name: buffer_name(n), lower: Some(Box::new(int(1))), upper: Some(Box::new(id(&length_name(n)))), stride: None };
            }
            Expr::Section { name, lower, upper, .. } if dummies.contains(name) => {
                let d = name.clone();
                *name = buffer_name(&d);
                if lower.is_none() { *lower = Some(Box::new(int(1))); }
                if upper.is_none() { *upper = Some(Box::new(id(&length_name(&d)))); }
            }
            _ => {}
        });
    };
    for e in crate::procedure_arguments::expressions(s) {
        rewrite(e);
    }
    for child in crate::procedure_arguments::children(s) {
        for s in child.iter_mut() {
            if !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
                references(s, dummies);
            }
        }
    }
}

fn statements(body: &mut Vec<Stmt>, converted: &HashMap<String, Converted>, failed: &mut bool, serial: &mut usize, slots: &mut Vec<String>) {
    let mut out = vec![];
    for mut s in std::mem::take(body) {
        if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }) {
            out.push(s);
            continue;
        }
        // Nested bodies first.
        for child in crate::procedure_arguments::children(&mut s) {
            statements(child, converted, failed, serial, slots);
        }
        let mut prefix = vec![];
        let mut pushed = 0;
        let controls = matches!(s, Stmt::Do { .. } | Stmt::DoWhile { .. }) || declaration(&s);
        if let Stmt::CallSub { name, args } = &mut s {
            if let Some(c) = converted.get(name.as_str()) {
                actuals(c, args, &mut prefix, &mut pushed, serial, slots);
                *name = c.clone.clone();
            }
        }
        for e in crate::procedure_arguments::expressions(&mut s) {
            crate::procedure_arguments::visit_expr(e, &mut |e| {
                if let Expr::Call(n, args) = e {
                    if let Some(c) = converted.get(n.as_str()) {
                        actuals(c, args, &mut prefix, &mut pushed, serial, slots);
                        *n = c.clone.clone();
                    }
                }
            });
        }
        if pushed == 0 {
            out.push(s);
            continue;
        }
        if controls {
            *failed = true;
            out.push(s);
            continue;
        }
        let pop = Stmt::Assign { name: TOP.into(), value: bin(BinOp::Sub, id(TOP), int(pushed as i64)) };
        out.extend(prefix);
        match &mut s {
            Stmt::If { then_body, else_body, .. } => {
                then_body.insert(0, pop.clone());
                else_body.get_or_insert_with(Vec::new).insert(0, pop);
                out.push(s);
            }
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases.iter_mut() { c.body.insert(0, pop.clone()); }
                default.get_or_insert_with(Vec::new).insert(0, pop);
                out.push(s);
            }
            _ => {
                out.push(s);
                out.push(pop);
            }
        }
    }
    *body = out;
}

/// Push the CHARACTER actuals of a call and pass their slots.
fn actuals(c: &Converted, args: &mut [Expr], prefix: &mut Vec<Stmt>, pushed: &mut usize, serial: &mut usize, slots: &mut Vec<String>) {
    for (position, arg) in args.iter_mut().enumerate() {
        // A keyword actual names its dummy.
        let value = match arg {
            Expr::Call(tag, values) if tag.starts_with("@keyword:") && values.len() == 1 => {
                let keyword = tag["@keyword:".len()..].to_owned();
                let Some((_, d)) = c.dummies.iter().find(|(_, d)| *d == keyword) else { continue };
                *tag = format!("@keyword:{}", slot_name(d));
                &mut values[0]
            }
            _ if c.dummies.iter().any(|(i, _)| *i == position) => arg,
            _ => continue,
        };
        *serial += 1;
        let slot = format!("rfort_rs_actual_{serial}");
        slots.push(slot.clone());
        prefix.push(Stmt::CallSub { name: PUSH.into(), args: vec![value.clone()] });
        prefix.push(Stmt::Assign { name: slot.clone(), value: id(TOP) });
        *value = id(&slot);
        *pushed += 1;
    }
}

/// The module of the string stack, used by every unit.
fn add_module(p: &mut Program) {
    let stop = |message: String| Stmt::CallSub { name: "@error_stop".into(), args: vec![Expr::Str(message)] };
    let length = Expr::Call("len".into(), vec![id("s")]);
    let end = bin(BinOp::Sub, bin(BinOp::Add, id("start"), length.clone()), int(1));
    let push_body = vec![
        Stmt::DummyIntent { name: "s".into(), intent: "intent(in)".into() },
        Stmt::VarDecl { kind: TypeSpec::CharacterAssumed, names: vec!["s".into()] },
        Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec!["start".into()] },
        Stmt::If {
            cond: bin(BinOp::Eq, id(TOP), int(0)),
            then_body: vec![Stmt::Assign { name: "start".into(), value: int(1) }],
            else_body: Some(vec![Stmt::Assign { name: "start".into(), value: bin(BinOp::Add,
                Expr::Call(STARTS.into(), vec![id(TOP)]), Expr::Call(LENGTHS.into(), vec![id(TOP)])) }]),
        },
        Stmt::If {
            cond: bin(BinOp::Ge, id(TOP), int(DEPTH as i64)),
            then_body: vec![stop(format!("more than {DEPTH} CHARACTER arguments of recursive procedures are pending"))],
            else_body: None,
        },
        Stmt::If {
            cond: bin(BinOp::Gt, length.clone(), int(BUFFER as i64)),
            then_body: vec![stop(format!("a CHARACTER argument of a recursive procedure is longer than {BUFFER} characters"))],
            else_body: None,
        },
        Stmt::If {
            cond: bin(BinOp::Gt, end.clone(), int(POOL as i64)),
            then_body: vec![stop(format!("more than {POOL} characters of arguments of recursive procedures are pending"))],
            else_body: None,
        },
        Stmt::Assign { name: TOP.into(), value: bin(BinOp::Add, id(TOP), int(1)) },
        Stmt::AssignIndex { name: STARTS.into(), indices: vec![id(TOP)], value: id("start") },
        Stmt::AssignIndex { name: LENGTHS.into(), indices: vec![id(TOP)], value: length.clone() },
        Stmt::If {
            cond: bin(BinOp::Gt, length, int(0)),
            then_body: vec![Stmt::AssignIndex { name: TEXT.into(), indices: vec![Expr::Section {
                name: TEXT.into(), lower: Some(Box::new(id("start"))), upper: Some(Box::new(end)), stride: None }], value: id("s") }],
            else_body: None,
        },
    ];
    let module = Stmt::Module {
        name: MODULE.into(),
        body: vec![
            Stmt::Implicit(crate::ast::Implicit::None),
            Stmt::VarDecl { kind: TypeSpec::Character(Some(POOL)), names: vec![TEXT.into()] },
            Stmt::ArrayDecl { kind: TypeSpec::Integer(None), name: STARTS.into(), dims: vec![int(DEPTH as i64)] },
            Stmt::ArrayDecl { kind: TypeSpec::Integer(None), name: LENGTHS.into(), dims: vec![int(DEPTH as i64)] },
            Stmt::InitializedDecl { kind: TypeSpec::Integer(None), entities: vec![(TOP.into(), Some(int(0)))], attributes: vec![] },
            Stmt::Subroutine { elemental: false, pure: false, name: PUSH.into(), params: vec!["s".into()], body: push_body },
        ],
    };
    let uses = Stmt::Use { module: MODULE.into(), nature: None, only: None, renames: vec![] };
    let mut main_use = false;
    for s in p.body.iter_mut() {
        match s {
            Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => body.insert(0, uses.clone()),
            _ => main_use = true,
        }
    }
    let mut body = vec![module];
    if main_use {
        let at = p.body.iter().position(|s| !matches!(s, Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::SourceLocation(_))).unwrap_or(p.body.len());
        p.body.insert(at, uses);
    }
    body.extend(std::mem::take(&mut p.body));
    p.body = body;
}

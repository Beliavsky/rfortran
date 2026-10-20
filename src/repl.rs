// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky

//! Sessions reuse the normal frontend and interpreter. Only declarations
//! and snapshots of initialized values survive; executable input is never replayed.
use crate::ast::{Expr, Implicit, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::io::{self, IsTerminal, Write};
use std::path::Path;

#[path = "repl_arrays.rs"]
mod arrays;
#[path = "repl_dynamic.rs"]
mod dynamic;
#[path = "repl_export.rs"]
mod export;
#[path = "repl_shortcuts.rs"]
mod shortcuts;
#[path = "repl_inspect.rs"]
mod inspect;

#[derive(serde::Serialize, serde::Deserialize)]
struct Evaluation {
    program: Program,
    flags: std::collections::BTreeMap<String, String>,
    seed: u64,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Snapshot {
    values: HashMap<String, Expr>,
    seed: u64,
}

/// Run submissions in a worker so runtime STOP/error exits cannot kill the
/// session, including release builds compiled with panic=abort.
fn execute(program: Program, flags: std::collections::BTreeMap<String,String>, seed: u64) -> Result<Snapshot> {
    struct Work(std::path::PathBuf);
    impl Drop for Work { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let directory = std::env::temp_dir().join(format!("rfort-repl-{}-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos(),
        SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir(&directory)?;
    let work = Work(directory);
    let path = work.0.join("input.json");
    std::fs::write(&path, serde_json::to_vec(&Evaluation { program, flags, seed })?)?;
    let status = std::process::Command::new(std::env::current_exe()?)
        .arg("repl-eval").arg(&path).stdin(std::process::Stdio::null()).status()?;
    if !status.success() { bail!("submission failed; previous scalar and array values retained"); }
    let result = std::fs::read(work.0.join("result.json"))?;
    Ok(serde_json::from_slice(&result)?)
}

pub(crate) fn evaluate(path: &Path) -> Result<()> {
    let mut input: Evaluation = serde_json::from_slice(&std::fs::read(path)?)?;
    let values = crate::runtime::interpret_scalars(&input.program, &input.flags, &mut input.seed)?;
    std::fs::write(path.with_file_name("result.json"), serde_json::to_vec(&Snapshot { values, seed: input.seed })?)?;
    Ok(())
}

struct Session {
    declarations: Vec<Stmt>,
    names: HashSet<String>,
    values: HashMap<String, Expr>,
    arrays: arrays::Values,
    allocations: dynamic::Allocations,
    automatic: bool,
    history: export::History,
    seed: u64,
}
impl Default for Session {
    fn default() -> Self {
        Self { declarations: vec![], names: HashSet::new(), values: HashMap::new(), arrays: HashMap::new(), allocations: Default::default(), automatic: false, history: Default::default(), seed: 1 }
    }
}

fn scalar(kind: &TypeSpec) -> Result<()> {
    if matches!(kind, TypeSpec::Complex(_)) { bail!("the scalar REPL does not yet support COMPLEX"); }
    Ok(())
}

fn validate(body: &[Stmt], top: bool) -> Result<()> {
    for stmt in body {
        match stmt {
            Stmt::SourceLocation(_) | Stmt::Assign { .. } | Stmt::AssignIndex { .. } | Stmt::Print { .. }
                | Stmt::Exit | Stmt::Cycle => {},
            Stmt::Implicit(Implicit::None) if top => {},
            Stmt::DummyIntent { intent, .. } if top && matches!(intent.as_str(), "parameter" | "allocatable") => {},
            Stmt::VarDecl { kind, .. } if top => scalar(kind)?,
            Stmt::ArrayDecl { kind, .. } if top => arrays::kind(kind)?,
            Stmt::InitializedDecl { kind, attributes, .. } if top => {
                scalar(kind)?;
                if attributes.iter().any(|a| !matches!(a.as_str(), "parameter" | "save")) {
                    bail!("the scalar REPL supports PARAMETER and SAVE declaration attributes only");
                }
            },
            Stmt::If { then_body, else_body, .. } => {
                validate(then_body, false)?;
                if let Some(body) = else_body { validate(body, false)?; }
            },
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => validate(body, false)?,
            Stmt::SelectCase { cases, default, .. } => {
                for case in cases { validate(&case.body, false)?; }
                if let Some(body) = default { validate(body, false)?; }
            },
            Stmt::CallSub { name, args } if name == "@write" => {
                if !matches!(args.first(), Some(Expr::IntLit(unit)) if unit == "6") {
                    bail!("scalar REPL WRITE currently requires standard output (*)");
                }
            },
            Stmt::CallSub { name, .. } if matches!(name.as_str(), "random_number" | "random_seed"
                | "@allocate" | "@deallocate"
                | "cpu_time" | "system_clock" | "@formatted_print"
                | "@formatted_write_noadvance") => {},
            _ => bail!("unsupported in the REPL: pointers, procedures, modules, READ, file operations, and program control statements; use rfort run for full programs"),
        }
    }
    Ok(())
}

impl Session {
    fn submit(&mut self, input: &str, args: &crate::cli::Cli) -> Result<()> {
        let tokens = crate::lexer::lex(input);
        let mut parsed = crate::parser::parse_with_src(input, &tokens, "<repl>")
            .map_err(|_| anyhow::anyhow!("parse failed"))?;
        validate(&parsed.body, true)?;
        let mut history = export::History::submission(input, &parsed.body)?;
        let mut declarations = self.declarations.clone();
        let mut names = self.names.clone();
        // Array initializers have a paired InitializedDecl and ArrayDecl.
        // Retain their shape once and execute mutable initializers only once.
        let array_names: HashSet<_> = parsed.body.iter().filter_map(|s| match s {
            Stmt::ArrayDecl { name, .. } => Some(name.clone()), _ => None,
        }).collect();
        for stmt in &parsed.body {
            if matches!(stmt, Stmt::DummyIntent { intent, .. } if matches!(intent.as_str(), "parameter" | "allocatable")) {
                declarations.push(stmt.clone());
                continue;
            }
            let (declared, retained) = match stmt {
                Stmt::VarDecl { names, .. } => (names.clone(), stmt.clone()),
                Stmt::ArrayDecl { name, .. } => (vec![name.clone()], stmt.clone()),
                Stmt::InitializedDecl { kind, attributes, entities } => {
                    let names: Vec<_> = entities.iter().map(|(n, _)| n.clone())
                        .filter(|n| !array_names.contains(n)).collect();
                    let retained = if attributes.iter().any(|a| a == "parameter") { stmt.clone() }
                        else { Stmt::VarDecl { kind: kind.clone(), names: names.clone() } };
                    (names, retained)
                },
                _ => continue,
            };
            for name in declared {
                if name.starts_with("rfort_repl_snapshot_") { bail!("name prefix `rfort_repl_snapshot_` is reserved by the REPL"); }
                if !names.insert(name.clone()) { bail!("`{name}` is already declared; use :reset to start a new session"); }
            }
            declarations.push(retained);
        }
        let mut body = vec![Stmt::Implicit(Implicit::None)];
        body.extend(self.declarations.clone());
        // Restore values, not earlier source statements or initializer calls.
        // Uninitialized variables have no snapshot and retain the usual checks.
        let mut values: Vec<_> = self.values.iter().collect();
        values.sort_by_key(|(name, _)| *name);
        body.extend(values.into_iter().map(|(name, value)| Stmt::Assign { name: name.clone(), value: value.clone() }));
        body.append(&mut parsed.body);
        let program = Program { name: "<repl>".into(), body, contains: vec![] };
        if args.standard.is_some() {
            crate::standards::check(&program, input, &tokens, "<repl>", None, args.standard, args.werror)?;
        }
        let prepared = dynamic::prepare(program, &self.allocations, &names, self.automatic)?;
        history.inferred(&prepared.inferred)?;
        for stmt in &prepared.inferred {
            match stmt {
                Stmt::VarDecl { names: declared, .. } => names.extend(declared.iter().cloned()),
                Stmt::ArrayDecl { name, .. } => { names.insert(name.clone()); },
                _ => {},
            }
        }
        declarations.extend(prepared.inferred.clone());
        let mut capture_names = names.clone();
        for name in prepared.allocations.keys() { capture_names.remove(name); }
        capture_names.extend(prepared.bindings.values().cloned());
        let mut program = crate::lower_program_body(prepared.program, input, &tokens, "<repl>", None)?;
        arrays::restore(&mut program, &self.arrays);
        let mut settings = crate::sema::SemaSettings::default();
        settings.werror = args.werror;
        crate::suppress_legacy_unused(&mut settings);
        let errors = crate::sema::analyze_with_src(&program, input, &tokens, "<repl>", &settings, &HashSet::new(), false);
        for error in &errors { eprintln!("{error}"); }
        if errors.iter().any(|e| matches!(e.kind, crate::errors::CompileErrorKind::Semantic)) {
            bail!("semantic checks failed");
        }
        let (mut program, mut flags) = crate::initialized::instrument_scalars(program)
            .map_err(|e| crate::source_diagnostics::report(e, input, &tokens, "<repl>", None))?;
        let capture = arrays::Capture::prepare(&mut program, &mut flags, &capture_names)?;
        let mut snapshot = execute(program, flags, self.seed)?;
        let mut saved = capture.finish(&mut snapshot.values)?;
        for (name, storage) in &prepared.bindings {
            if name != storage { if let Some(value) = saved.remove(storage) { saved.insert(name.clone(), value); } }
        }
        self.arrays = saved;
        self.allocations = prepared.allocations;
        self.values = snapshot.values;
        self.seed = snapshot.seed;
        self.declarations = declarations;
        self.names = names;
        self.history.append(history);
        Ok(())
    }
}

fn wrapped(input: &str, expression: bool) -> String {
    format!("implicit none\n{}{input}\nend\n", if expression { "print *, " } else { "" })
}

/// Strip comments without treating ! inside a quoted string as a comment.
fn code(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quote = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if Some(b) == quote {
            if bytes.get(i + 1) == Some(&b) { i += 2; continue; }
            quote = None;
        } else if quote.is_none() {
            if b == b'!' { return &line[..i]; }
            if b == b'\'' || b == b'"' { quote = Some(b); }
        }
        i += 1;
    }
    line
}

pub(crate) fn run(args: &crate::cli::Cli) -> Result<()> {
    let terminal = io::stdin().is_terminal();
    if terminal { println!("rfort REPL. :help for help; :quit to exit."); }
    let mut session = Session::default();
    let mut pending = String::new();
    loop {
        if terminal {
            print!("{}", if pending.is_empty() { "rfort> " } else { "   ... " });
            io::stdout().flush()?;
        }
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            if !pending.is_empty() { eprintln!("Error: incomplete input at end of session; submission discarded"); }
            break;
        }
        let command = shortcuts::command(line.trim());
        match command.as_str() {
            ":quit" | ":q" => break,
            ":reset" => { session = Session::default(); pending.clear(); continue; },
            ":cancel" => { pending.clear(); continue; },
            ":auto" => { println!("automatic declarations: {}", if session.automatic { "on" } else { "off" }); continue; },
            ":auto on" => { session.automatic = true; continue; },
            ":auto off" => { session.automatic = false; continue; },
            ":info" => {
                for line in dynamic::variables(&session.declarations, &session.allocations) { println!("{line}"); }
                continue;
            },
            text if matches!(text.split_whitespace().next(), Some(":vars" | ":shapes" | ":sizes")) => {
                let result = (|| -> Result<()> {
                    let end = text.find(char::is_whitespace).unwrap_or(text.len());
                    let operation = &text[..end];
                    let arguments = &text[end..];
                    let view = inspect::View::new(&session)?;
                    let mut selected = shortcuts::selected(arguments, &session.names)?;
                    if arguments.trim().is_empty() && operation != ":vars" { selected.retain(|n| view.shapes.contains_key(n)); }
                    let mut lines = vec![];
                    for name in selected {
                        let value = if operation == ":vars" { view.value(&name, &session) }
                            else { view.shape(&name, &session, operation == ":sizes")? };
                        lines.push(format!("{name} = {value}"));
                    }
                    for line in lines { println!("{line}"); }
                    Ok(())
                })();
                if let Err(error) = result { eprintln!("Error: {error:#}"); }
                continue;
            },
            ":list" | ":list -n" => {
                let source = session.history.source(&session.names);
                if command == ":list -n" { print!("{source}"); }
                else { for (number, line) in source.lines().enumerate() { println!("{:4} {line}", number + 1); } }
                continue;
            },
            text if text == ":save" || text.starts_with(":save ") || text.starts_with(":save\t") => {
                let result = (|| -> Result<()> {
                    if !pending.is_empty() { bail!("finish or :cancel pending input before saving the session"); }
                    session.history.save(export::save_path(text)?, &session.names)
                })();
                if let Err(error) = result { eprintln!("Error: {error:#}"); }
                continue;
            },
            ":help" => {
                println!("Declare scalars or INTEGER/REAL/LOGICAL arrays, assign values, PRINT/WRITE, or enter an expression.\nArrays support indexing, sections and expressions (snapshot limit: 10000 elements per array).\nMultiline IF/DO/SELECT CASE blocks and & continuations are accepted.\nStrict mode requires declarations (IMPLICIT NONE). :auto on enables inferred declarations for top-level assignments; :auto off restores strict mode.\n:save filename.f90 exports successful input as a compilable program; quote filenames containing spaces.\n.vars/:vars shows saved values; .info/:info shows declarations and allocation sizes.\n.list [-n] lists exported source; .shapes and .sizes inspect arrays.\nLET name = expression declares a new variable; CONST name = expression declares a named constant.\nDot commands and colon commands are interchangeable; .clear resets the session. Rank-one allocatable arrays can resize on assignment at the prompt; ALLOCATE requires a constant extent.\n:reset clears the session; :cancel discards pending input; :quit exits.\nPointer/CHARACTER arrays, modules, procedures, READ and file I/O are not supported yet.");
                continue;
            },
            text if text.starts_with(':') => { eprintln!("Error: unknown REPL command `{text}`; use :help"); continue; },
            _ => {},
        }
        if code(&line).trim().is_empty() { continue; }
        let continued = code(&line).trim_end().ends_with('&');
        pending.push_str(&line);
        if !line.ends_with('\n') { pending.push('\n'); }
        if continued { continue; }
        let expanded = match shortcuts::expand(&pending, &session) {
            Ok(source) => source,
            Err(error) => { eprintln!("Error: {error:#}"); pending.clear(); continue; },
        };
        let source = expanded.as_deref().unwrap_or(&pending);
        let statement = wrapped(source, false);
        let expression = wrapped(source, true);
        let stmt_probe = crate::parser::probe_fragment(&statement, statement.len() - 4);
        let expr_probe = crate::parser::probe_fragment(&expression, expression.len() - 4);
        let selected = if stmt_probe.0 { statement }
            else if expr_probe.0 { expression }
            else if stmt_probe.1 { continue; }
            else { statement };
        if let Err(error) = session.submit(&selected, args) {
            eprintln!("Error: {}", crate::source_diagnostics::user_names(&format!("{error:#}")));
        }
        pending.clear();
    }
    Ok(())
}

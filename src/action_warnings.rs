// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Waction-mismatch`, on by default: a READ from a constant unit opened
//! with ACTION='WRITE', or a WRITE to one opened with ACTION='READ', on every
//! path reaching the statement. Either is an error when the statement runs.
//!
//! Units are integer literals or local named constants. An OPEN with a
//! constant ACTION of READ or WRITE, and without IOSTAT= or ERR=, gives its
//! unit that action; CLOSE, any other OPEN of the unit, an OPEN of a unit the
//! analysis cannot resolve, and a CALL of a procedure that is not a known
//! intrinsic make it unknown. Only an action that is certain on every path
//! is reported. Transfers with IOSTAT= or ERR= handle the error and are not
//! reported, and procedures with GO TO or with ERR= or END= branches are not
//! checked.
use crate::ast::{Expr, Program, Stmt};
use crate::leak_warnings::{gotos, span, INTRINSICS};
use crate::lexer::{lex, Token};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap};

const CATEGORY: &str = "action-mismatch";

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum State {
    /// Connected with ACTION='READ' (false) or 'WRITE' (true) by the OPEN
    /// at this offset.
    Known(bool, usize),
    Unknown,
}
type States = BTreeSet<State>;

#[derive(Default)]
struct Flow {
    normal: States,
    exits: States,
    cycles: States,
}

/// The value of an `@io_spec:key` control.
fn spec<'a>(args: &'a [Expr], key: &str) -> Option<&'a Expr> {
    args.iter().find_map(|a| match a {
        Expr::Call(n, v) if n.strip_prefix("@io_spec:") == Some(key) && v.len() == 1 => Some(&v[0]),
        _ => None,
    })
}

/// The unit of OPEN or CLOSE: UNIT= or the first positional item.
fn io_unit(args: &[Expr]) -> Option<&Expr> {
    spec(args, "unit").or_else(|| args.first().filter(|a| !matches!(a, Expr::Call(n, _) if n.starts_with("@io_spec:"))))
}

/// A data transfer: the unit, whether it writes, and whether the statement
/// has a control that handles an error (IOSTAT= or ERR=) or branches (END=).
fn transfer(s: &Stmt) -> Option<(&Expr, bool, bool)> {
    let Stmt::CallSub { name, args } = s else { return None };
    let unit = args.first()?;
    match name.as_str() {
        "@write" | "@formatted_write_noadvance" => Some((unit, true,
            args.iter().any(|a| matches!(a, Expr::Call(n, _) if n.starts_with("@write_option:") && !n.ends_with(":advance"))))),
        "@unformatted_write" => Some((unit, true, !matches!(args.get(1), Some(Expr::Str(c)) if c.is_empty()))),
        "@formatted_read" => Some((unit, false, false)),
        "@internal_read" | "@empty_read" | "@unformatted_read" => Some((unit, false, !matches!(args.get(1), Some(Expr::Str(c)) if c.is_empty()))),
        "@formatted_read_status" => Some((unit, false, !matches!(args.get(2), Some(Expr::Str(c)) if c.is_empty()))),
        _ => None,
    }
}

/// Whether a statement may branch on an I/O condition: ERR= or END= on a
/// transfer, OPEN or CLOSE.
fn branches(body: &[Stmt]) -> bool {
    body.iter().any(|s| {
        let label = |k: &str| k == "err" || k == "end" || k == "eor";
        let direct = match s {
            Stmt::CallSub { name, args } if name == "@io_open" || name == "@io_close" => {
                args.iter().any(|a| matches!(a, Expr::Call(n, _) if n.strip_prefix("@io_spec:").is_some_and(label)))
            }
            Stmt::CallSub { name, args } if transfer(s).is_some() => match name.as_str() {
                "@write" | "@formatted_write_noadvance" => args.iter().any(|a| matches!(a, Expr::Call(n, _) if n.strip_prefix("@write_option:").is_some_and(label))),
                "@formatted_read_status" => matches!(args.get(2), Some(Expr::Str(c)) if label(&c.to_ascii_lowercase())),
                _ => matches!(args.get(1), Some(Expr::Str(c)) if label(&c.to_ascii_lowercase())),
            },
            _ => false,
        };
        direct || {
            let mut copy = s.clone();
            !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. })
                && crate::procedure_arguments::children(&mut copy).into_iter().any(|b| branches(b))
        }
    })
}

/// Local named INTEGER constants with a literal value.
fn constants(body: &[Stmt]) -> HashMap<String, i64> {
    let mut out = HashMap::new();
    for s in body {
        if let Stmt::InitializedDecl { attributes, entities, .. } = s {
            if attributes.iter().any(|a| a.eq_ignore_ascii_case("parameter")) {
                for (n, v) in entities {
                    if let Some(Expr::IntLit(v)) = v {
                        if let Ok(v) = v.parse() { out.insert(n.clone(), v); }
                    }
                }
            }
        }
    }
    out
}

struct Walker<'a> {
    unit: i64,
    constants: &'a HashMap<String, i64>,
    /// The states reaching each READ (false) or WRITE (true) on the unit
    /// without IOSTAT= or ERR=, over every pass of the loops.
    records: BTreeMap<(usize, bool), States>,
    diverged: bool,
}

impl Walker<'_> {
    fn value(&self, e: &Expr) -> Option<i64> {
        match e {
            Expr::IntLit(v) => v.parse().ok(),
            Expr::Ident(n) => self.constants.get(n).copied(),
            _ => None,
        }
    }

    fn looped(&mut self, body: &[Stmt], entry: States, offset: usize, endless: bool) -> Flow {
        let mut head = entry.clone();
        let mut exits = States::new();
        let mut settled = false;
        for _ in 0..32 {
            let f = self.walk(body, head.clone(), offset);
            exits.extend(f.exits);
            let mut next = entry.clone();
            next.extend(f.normal);
            next.extend(f.cycles);
            if next == head {
                settled = true;
                break;
            }
            head = next;
        }
        self.diverged |= !settled;
        let mut normal = if endless { States::new() } else { head };
        normal.extend(exits);
        Flow { normal, ..Flow::default() }
    }

    fn walk(&mut self, body: &[Stmt], input: States, mut offset: usize) -> Flow {
        let mut cur = input;
        let mut flow = Flow::default();
        for s in body {
            if let Stmt::SourceLocation(n) = s {
                offset = *n;
                continue;
            }
            if cur.is_empty() {
                continue;
            }
            if let Some((unit, writes, handled)) = transfer(s) {
                if !handled && self.value(unit) == Some(self.unit) {
                    self.records.entry((offset, writes)).or_default().extend(cur.iter().cloned());
                }
                continue;
            }
            match s {
                Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. } => {}
                Stmt::CallSub { name, args } if name == "@io_open" => {
                    if spec(args, "newunit").is_some() {
                        continue;
                    }
                    match io_unit(args).and_then(|u| self.value(u)) {
                        Some(u) if u != self.unit => {}
                        // An OPEN of a unit the analysis cannot resolve might
                        // connect this one.
                        None => cur = States::from([State::Unknown]),
                        Some(_) => {
                            let action = match spec(args, "action") {
                                Some(Expr::Str(a)) => match a.trim_end().to_ascii_lowercase().as_str() {
                                    "read" => Some(false),
                                    "write" => Some(true),
                                    _ => None,
                                },
                                _ => None,
                            };
                            cur = States::from([match action {
                                Some(w) if spec(args, "iostat").is_none() => State::Known(w, offset),
                                _ => State::Unknown,
                            }]);
                        }
                    }
                }
                Stmt::CallSub { name, args } if name == "@io_close" => {
                    if io_unit(args).and_then(|u| self.value(u)).is_none_or(|u| u == self.unit) {
                        cur = States::from([State::Unknown]);
                    }
                }
                Stmt::CallSub { name, .. } if name == "@stop" || name == "@error_stop" => cur.clear(),
                // A procedure might close or reconnect the unit.
                Stmt::CallSub { name, .. } if !name.starts_with('@') && !INTRINSICS.contains(&name.to_ascii_lowercase().as_str()) => {
                    cur = States::from([State::Unknown]);
                }
                Stmt::Return(_) => cur.clear(),
                Stmt::Exit => flow.exits.extend(std::mem::take(&mut cur)),
                Stmt::Cycle => flow.cycles.extend(std::mem::take(&mut cur)),
                Stmt::If { then_body, else_body, .. } => {
                    let t = self.walk(then_body, cur.clone(), offset);
                    let e = match else_body {
                        Some(b) => self.walk(b, cur.clone(), offset),
                        None => Flow { normal: cur.clone(), ..Flow::default() },
                    };
                    cur = t.normal;
                    cur.extend(e.normal);
                    for f in [t.exits, e.exits] { flow.exits.extend(f); }
                    for f in [t.cycles, e.cycles] { flow.cycles.extend(f); }
                }
                Stmt::SelectCase { cases, default, .. } => {
                    let mut out = if default.is_none() { cur.clone() } else { States::new() };
                    for b in cases.iter().map(|c| &c.body).chain(default.iter()) {
                        let f = self.walk(b, cur.clone(), offset);
                        out.extend(f.normal);
                        flow.exits.extend(f.exits);
                        flow.cycles.extend(f.cycles);
                    }
                    cur = out;
                }
                Stmt::Do { body, .. } => cur = self.looped(body, cur, offset, false).normal,
                Stmt::DoWhile { cond, body } => cur = self.looped(body, cur, offset, matches!(cond, Expr::Logical(true))).normal,
                // An explicit-step DO or FORALL is a BLOCK that starts with its control.
                Stmt::Block { body } if matches!(body.first(), Some(Stmt::CallSub { name, .. }) if name == "@do_step" || name == "@forall") => {
                    cur = self.looped(&body[1..], cur, offset, false).normal;
                }
                Stmt::Block { body } => {
                    let f = self.walk(body, cur, offset);
                    cur = f.normal;
                    flow.exits.extend(f.exits);
                    flow.cycles.extend(f.cycles);
                }
                _ => {
                    // Other constructs, such as SELECT TYPE or ASSOCIATE
                    // after their lowering, are not followed: a unit they
                    // reconnect becomes unknown.
                    let mut copy = s.clone();
                    if crate::procedure_arguments::children(&mut copy).into_iter().any(|b| self.touches(b)) {
                        cur = States::from([State::Unknown]);
                    }
                }
            }
        }
        flow.normal = cur;
        flow
    }

    /// Whether a body might OPEN or CLOSE the unit or call a procedure.
    fn touches(&self, body: &[Stmt]) -> bool {
        body.iter().any(|s| match s {
            Stmt::CallSub { name, args } if name == "@io_open" || name == "@io_close" => {
                spec(args, "newunit").is_none() && io_unit(args).and_then(|u| self.value(u)).is_none_or(|u| u == self.unit)
            }
            Stmt::CallSub { name, .. } => !name.starts_with('@') && !INTRINSICS.contains(&name.to_ascii_lowercase().as_str()),
            _ => {
                let mut copy = s.clone();
                crate::procedure_arguments::children(&mut copy).into_iter().any(|b| self.touches(b))
            }
        })
    }
}

struct Report {
    unit: i64,
    offset: usize,
    writes: bool,
    opens: Vec<usize>,
}

/// Constant units that an OPEN in the body, outside internal procedures,
/// connects with ACTION='READ' or 'WRITE'.
fn units(body: &[Stmt], constants: &HashMap<String, i64>, out: &mut BTreeSet<i64>) {
    for s in body {
        match s {
            Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. } => {}
            Stmt::CallSub { name, args } if name == "@io_open" && spec(args, "action").is_some() => {
                let value = match io_unit(args) {
                    Some(Expr::IntLit(v)) => v.parse().ok(),
                    Some(Expr::Ident(n)) => constants.get(n).copied(),
                    _ => None,
                };
                out.extend(value);
            }
            _ => {
                let mut copy = s.clone();
                for b in crate::procedure_arguments::children(&mut copy) { units(b, constants, out); }
            }
        }
    }
}

fn analyze(body: &[Stmt], out: &mut Vec<Report>) {
    if gotos(body) || branches(body) {
        return;
    }
    let constants = constants(body);
    let mut candidates = BTreeSet::new();
    units(body, &constants, &mut candidates);
    for unit in candidates {
        let mut w = Walker { unit, constants: &constants, records: BTreeMap::new(), diverged: false };
        w.walk(body, States::from([State::Unknown]), 0);
        if w.diverged {
            continue;
        }
        for ((offset, writes), states) in w.records {
            // A READ is an error under ACTION='WRITE', a WRITE under 'READ'.
            let opens: Option<Vec<usize>> = states.iter().map(|s| match s {
                State::Known(w, at) if *w != writes => Some(*at),
                _ => None,
            }).collect();
            if let Some(opens) = opens.filter(|o| !o.is_empty()) {
                out.push(Report { unit, offset, writes, opens });
            }
        }
    }
}

fn procedures(body: &[Stmt], out: &mut Vec<Report>) {
    for s in body {
        match s {
            Stmt::Module { body, .. } => procedures(body, out),
            Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => {
                procedures(body, out);
                analyze(body, out);
            }
            _ => {}
        }
    }
}

pub(crate) fn check(
    program: &Program,
    src: &str,
    own_len: usize,
    filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
    settings: &SemaSettings,
    options: &[String],
) -> Result<()> {
    // On by default: only -Wno-action-mismatch or !#allow(action-mismatch) turn it off.
    let (mut enabled, error) = crate::unused::selected(CATEGORY, false, settings, options);
    let disabled = options.iter().rev().find(|o| o.trim_start_matches("no-") == CATEGORY).is_some_and(|o| o.starts_with("no-"));
    if !disabled && !settings.allow.contains(CATEGORY) { enabled = true; }
    if !enabled { return Ok(()); }
    let mut reports = vec![];
    procedures(&program.body, &mut reports);
    analyze(&program.body, &mut reports);
    reports.retain(|r| r.offset < own_len);
    if reports.is_empty() { return Ok(()); }
    reports.sort_by_key(|r| (r.offset, r.unit));
    let tokens: Vec<Token> = lex(src);
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFiles,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let mut files = SimpleFiles::new();
    let fallback = files.add(filename, src);
    let ids = origins.iter().map(|o| files.add(o.name.as_str(), o.source(src))).collect::<Vec<_>>();
    let locate = |span: std::ops::Range<usize>| match origins.iter().enumerate().find(|(_, o)| o.range.contains(&span.start)) {
        Some((i, o)) => (ids[i], o.span(span)),
        None => (fallback, span.start.min(src.len())..span.end.min(src.len())),
    };
    for Report { unit, offset, writes, opens } in &reports {
        let (statement, action) = if *writes { ("WRITE to", "READ") } else { ("READ from", "WRITE") };
        let (file, at) = locate(span(src, &tokens, *offset, None));
        let mut labels = vec![Label::primary(file, at).with_message(format!("unit {unit} is connected with ACTION='{action}' here"))];
        for at in opens {
            let (file, at) = locate(span(src, &tokens, *at, None));
            labels.push(Label::secondary(file, at).with_message(format!("opened with ACTION='{action}' here")));
        }
        let d = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(format!("{statement} unit {unit}, which is opened with ACTION='{action}', is an error at run time [{CATEGORY}]"))
            .with_labels(labels)
            .with_notes(vec![format!("open the unit with ACTION='READWRITE', or add IOSTAT= to handle the error; -Wno-{CATEGORY} disables this warning")]);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &d)?;
    }
    if error { bail!("{CATEGORY} warnings treated as errors"); }
    Ok(())
}

#[cfg(test)]
mod tests {
    fn reports(src: &str) -> Vec<(i64, bool)> {
        let program = crate::parser::parse_with_src(src, &crate::lexer::lex(src), "t.f90").unwrap_or_else(|_| panic!("{src}"));
        let mut out = vec![];
        super::procedures(&program.body, &mut out);
        super::analyze(&program.body, &mut out);
        out.iter().map(|r| (r.unit, r.writes)).collect()
    }

    #[test]
    fn straight_line_mismatches() {
        assert_eq!(reports("integer, parameter :: iu = 20\ninteger :: i\nopen (unit=iu, file='t.txt', action='write')\nread (iu,*) i\nend\n"), [(20, false)]);
        assert_eq!(reports("open (20, file='t.txt', action='read')\nwrite (20,*) 234\nend\n"), [(20, true)]);
    }

    #[test]
    fn uncertain_actions_are_not_reported() {
        // IOSTAT= on the transfer, a CLOSE and reopen, a branch, a call, and
        // OPEN without ACTION or with IOSTAT=.
        for src in [
            "integer :: i, ios\nopen (20, file='t', action='write')\nread (20,*,iostat=ios) i\nend\n",
            "integer :: i\nopen (20, file='t', action='write')\nclose (20)\nopen (20, file='t', action='read')\nread (20,*) i\nend\n",
            "integer :: i\nlogical :: c\nif (c) then\nopen (20, file='t', action='write')\nelse\nopen (20, file='t', action='read')\nend if\nread (20,*) i\nend\n",
            "integer :: i\nopen (20, file='t', action='write')\ncall reset()\nread (20,*) i\nend\n",
            "integer :: i\nopen (20, file='t')\nread (20,*) i\nend\n",
            "integer :: i, ios\nopen (20, file='t', action='write', iostat=ios)\nread (20,*) i\nend\n",
        ] {
            assert!(reports(src).is_empty(), "{src}");
        }
    }

    #[test]
    fn mismatches_on_every_path_and_in_loops() {
        assert_eq!(reports("integer :: i\nlogical :: c\nif (c) then\nopen (20, file='t', action='write')\nelse\nopen (20, file='t', action='write')\nend if\nread (20,*) i\nend\n"), [(20, false)]);
        assert_eq!(reports("integer :: i, k\nopen (20, file='t', action='read')\ndo k = 1, 3\nwrite (20,*) k\nend do\nend\n"), [(20, true)]);
        assert_eq!(reports("subroutine s()\nopen (21, file='t', action='read')\nwrite (21,'(a)') 'x'\nend subroutine\n"), [(21, true)]);
    }
}

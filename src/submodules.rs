// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Submodules (Fortran 2008) compiled together with their ancestor module.
//!
//! Before lexing, each submodule is merged into the text of its ancestor
//! module. A separate module procedure body replaces its interface body and
//! becomes a module procedure; the submodule's own declarations and procedures
//! become PRIVATE module entities. A submodule's scope extends its module's, so
//! host association then gives its procedures what the standard gives them.
//! Origins keep diagnostics on the original lines. A submodule compiled apart
//! from its module is not yet supported.
use crate::source_diagnostics::SourceOrigin;
use crate::source_includes::Input;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

struct Word {
    text: String,
    at: usize,
}

struct Statement {
    /// The text from the end of the previous statement through this one's end
    /// of line (or `;`), so consecutive statements tile the source.
    lines: Range<usize>,
    /// Lowercase names and keywords, literal strings and punctuation, without
    /// a statement label, comments or continuation marks.
    words: Vec<Word>,
}

impl Statement {
    fn word(&self, i: usize) -> &str {
        self.words.get(i).map_or("", |w| w.text.as_str())
    }
    fn span(&self, i: usize) -> Range<usize> {
        self.words.get(i).map_or(self.lines.start..self.lines.start, |w| w.at..w.at + w.text.len())
    }
}

fn ident(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_alphabetic())
}

/// The statements of free-form source.
fn statements(text: &str) -> Vec<Statement> {
    fn finish(out: &mut Vec<Statement>, region: &mut usize, end: usize, words: &mut Vec<Word>) {
        if words.is_empty() {
            return;
        }
        let mut words = std::mem::take(words);
        if words.len() > 1 && words[0].text.bytes().all(|c| c.is_ascii_digit()) {
            words.remove(0);
        }
        out.push(Statement { lines: *region..end, words });
        *region = end;
    }
    let b = text.as_bytes();
    let blank = |c: u8| c == b' ' || c == b'\t' || c == b'\r';
    let (mut out, mut words, mut region, mut i) = (Vec::new(), Vec::new(), 0, 0);
    while i < b.len() {
        let c = b[i];
        match c {
            b'\n' | b';' => {
                i += 1;
                finish(&mut out, &mut region, i, &mut words);
            }
            b'!' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'&' => {
                let mut j = i + 1;
                while j < b.len() && blank(b[j]) {
                    j += 1;
                }
                if j < b.len() && b[j] != b'\n' && b[j] != b'!' {
                    words.push(Word { text: "&".into(), at: i });
                    i += 1;
                    continue;
                }
                // A continuation resumes after the next line's optional `&`,
                // past blank and comment lines.
                loop {
                    while j < b.len() && b[j] != b'\n' {
                        j += 1;
                    }
                    if j >= b.len() {
                        break;
                    }
                    j += 1;
                    let mut k = j;
                    while k < b.len() && blank(b[k]) {
                        k += 1;
                    }
                    if k < b.len() && (b[k] == b'!' || b[k] == b'\n') {
                        j = k;
                        continue;
                    }
                    j = if k < b.len() && b[k] == b'&' { k + 1 } else { k };
                    break;
                }
                i = j;
            }
            b'\'' | b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'\n' {
                    if b[i] == c {
                        if b.get(i + 1) == Some(&c) {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                words.push(Word { text: text[start..i].into(), at: start });
            }
            c if blank(c) => i += 1,
            c if c.is_ascii_alphanumeric() || c == b'_' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                words.push(Word { text: text[start..i].to_ascii_lowercase(), at: start });
            }
            _ => {
                let start = i;
                i += if text[i..].starts_with("::") || text[i..].starts_with("=>") {
                    2
                } else {
                    text[i..].chars().next().map_or(1, char::len_utf8)
                };
                words.push(Word { text: text[start..i].into(), at: start });
            }
        }
    }
    finish(&mut out, &mut region, b.len(), &mut words);
    out
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Module(String),
    Submodule { ancestor: String, parent: Option<String>, name: String },
    Program,
    /// A function or subroutine; `separate` when MODULE is among its prefixes.
    Subprogram { name: String, function: bool, separate: bool },
    /// `MODULE PROCEDURE name`, a separate module procedure body.
    Procedure(String),
    /// An interface block, with its generic name (empty for an operator or
    /// assignment) if it is generic.
    Interface { generic: Option<String> },
    Type(String),
    EndInterface,
    EndType,
    Contains,
    /// The END of a program unit or subprogram.
    End,
    Use,
    Import,
    Implicit,
    Other,
}

/// The index after the parenthesis that closes the one at `open`.
fn closing(s: &Statement, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, w) in s.words.iter().enumerate().skip(open) {
        match w.text.as_str() {
            "(" => depth += 1,
            ")" => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// The index of the FUNCTION or SUBROUTINE keyword of a subprogram statement,
/// and whether MODULE is among its prefixes.
fn subprogram_keyword(s: &Statement) -> Option<(usize, bool)> {
    let (mut i, mut separate) = (0, false);
    loop {
        match s.word(i) {
            "module" => {
                separate = true;
                i += 1;
            }
            "pure" | "impure" | "elemental" | "recursive" | "non_recursive" | "simple" | "precision" => i += 1,
            "integer" | "real" | "complex" | "logical" | "character" | "type" | "class" | "double"
            | "doubleprecision" | "doublecomplex" => {
                i += 1;
                if s.word(i) == "*" {
                    i += 1;
                    i = if s.word(i) == "(" { closing(s, i)? } else { i + 1 };
                } else if s.word(i) == "(" {
                    i = closing(s, i)?;
                }
            }
            "function" | "subroutine" => return ident(s.word(i + 1)).then_some((i, separate)),
            _ => return None,
        }
    }
}

fn classify(s: &Statement, in_interface: bool) -> Kind {
    let w = |i| s.word(i);
    let n = s.words.len();
    match w(0) {
        "module" if n == 2 && w(1) != "procedure" && ident(w(1)) => return Kind::Module(w(1).into()),
        "module" if n == 3 && w(1) == "procedure" && !in_interface && ident(w(2)) => return Kind::Procedure(w(2).into()),
        "submodule" if w(1) == "(" => {
            let (ancestor, parent, close) = if w(3) == ":" { (w(2), Some(w(4).to_string()), 5) } else { (w(2), None, 3) };
            if w(close) == ")" && ident(w(close + 1)) && n == close + 2 {
                return Kind::Submodule { ancestor: ancestor.into(), parent, name: w(close + 1).into() };
            }
        }
        "program" if n == 2 => return Kind::Program,
        "block" if w(1) == "data" => return Kind::Program,
        "blockdata" => return Kind::Program,
        "interface" if n == 1 || ident(w(1)) => {
            let generic = (n > 1).then(|| if n == 2 { w(1).to_string() } else { String::new() });
            return Kind::Interface { generic };
        }
        "abstract" if w(1) == "interface" && n == 2 => return Kind::Interface { generic: None },
        "type" if !(w(1) == "is" && w(2) == "(") && w(1) != "(" => {
            let name = s.words.iter().position(|x| x.text == "::").map_or(w(1), |p| s.word(p + 1));
            if ident(name) {
                return Kind::Type(name.into());
            }
        }
        "contains" if n == 1 => return Kind::Contains,
        "use" => return Kind::Use,
        "import" => return Kind::Import,
        "implicit" => return Kind::Implicit,
        first if first == "end" || first.starts_with("end") && n <= 2 => {
            let what = if first == "end" {
                if w(1) == "block" && w(2) == "data" { "blockdata" } else { w(1) }
            } else {
                &first[3..]
            };
            match what {
                "" => return Kind::End,
                "interface" => return Kind::EndInterface,
                "type" => return Kind::EndType,
                "module" | "submodule" | "function" | "subroutine" | "procedure" | "program" | "blockdata" => return Kind::End,
                _ if first == "end" => return Kind::Other,
                _ => {}
            }
        }
        _ => {}
    }
    match subprogram_keyword(s) {
        Some((i, separate)) => Kind::Subprogram { name: w(i + 1).into(), function: w(i) == "function", separate },
        None => Kind::Other,
    }
}

struct Node {
    kind: Kind,
    open: usize,
    close: usize,
    contains: Option<usize>,
    children: Vec<Node>,
}

impl Node {
    fn spec_end(&self) -> usize {
        self.contains.unwrap_or(self.close)
    }
}

/// The program units, with their subprograms, interface blocks and type
/// definitions; None when the units do not nest.
fn tree(stmts: &[Statement]) -> Option<Vec<Node>> {
    let (mut stack, mut roots): (Vec<Node>, Vec<Node>) = (vec![], vec![]);
    let mut unit_start = 0;
    for (k, s) in stmts.iter().enumerate() {
        let in_interface = matches!(stack.last(), Some(Node { kind: Kind::Interface { .. }, .. }));
        let kind = classify(s, in_interface);
        match kind {
            Kind::Module(_) | Kind::Submodule { .. } | Kind::Program | Kind::Subprogram { .. } | Kind::Procedure(_)
            | Kind::Interface { .. } | Kind::Type(_) => {
                stack.push(Node { kind, open: k, close: k, contains: None, children: vec![] })
            }
            Kind::Contains => {
                if let Some(top) = stack.last_mut() {
                    top.contains.get_or_insert(k);
                }
            }
            Kind::End | Kind::EndInterface | Kind::EndType => {
                // A main program without a PROGRAM statement.
                let mut node = match stack.pop() {
                    Some(node) => node,
                    None if kind == Kind::End => Node { kind: Kind::Program, open: unit_start, close: k, contains: None, children: vec![] },
                    None => return None,
                };
                let matching = match kind {
                    Kind::EndInterface => matches!(node.kind, Kind::Interface { .. }),
                    Kind::EndType => matches!(node.kind, Kind::Type(_)),
                    _ => !matches!(node.kind, Kind::Interface { .. } | Kind::Type(_)),
                };
                if !matching {
                    return None;
                }
                node.close = k;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => {
                        roots.push(node);
                        unit_start = k + 1;
                    }
                }
            }
            _ => {}
        }
    }
    stack.is_empty().then_some(roots)
}

enum Item<'a> {
    Statement(usize),
    Node(&'a Node),
}

/// The statements and nested constructs of `node` from statement `from` up to
/// statement `to`.
fn items(node: &Node, from: usize, to: usize) -> Vec<Item<'_>> {
    let mut out = vec![];
    let mut children = node.children.iter().peekable();
    let mut k = from;
    while k < to {
        while children.peek().is_some_and(|c| c.open < k) {
            children.next();
        }
        match children.peek() {
            Some(child) if child.open == k => {
                out.push(Item::Node(child));
                k = child.close + 1;
                children.next();
            }
            _ => {
                out.push(Item::Statement(k));
                k += 1;
            }
        }
    }
    out
}

/// The names a declaration statement declares.
fn declared(s: &Statement) -> Vec<String> {
    let w = |i| s.word(i);
    let mut i = match w(0) {
        "integer" | "real" | "complex" | "logical" | "character" | "double" | "doubleprecision" | "doublecomplex"
        | "type" | "class" | "procedure" | "enumerator" => 1,
        "parameter" if w(1) == "(" => {
            return s.words.windows(2).filter(|p| p[1].text == "=" && ident(&p[0].text)).map(|p| p[0].text.clone()).collect();
        }
        _ => return vec![],
    };
    if let Some(p) = s.words.iter().position(|x| x.text == "::") {
        i = p + 1;
    } else {
        if w(0) == "double" {
            i = 2;
        }
        if w(i) == "*" {
            i = if w(i + 1) == "(" { closing(s, i + 1).unwrap_or(s.words.len()) } else { i + 2 };
        } else if w(i) == "(" {
            i = closing(s, i).unwrap_or(s.words.len());
        }
    }
    let (mut names, mut depth, mut expect) = (vec![], 0, true);
    for x in s.words.iter().skip(i) {
        match x.text.as_str() {
            "(" | "[" => depth += 1,
            ")" | "]" => depth -= 1,
            "," if depth == 0 => {
                expect = true;
                continue;
            }
            t if expect && depth == 0 && ident(t) => names.push(t.to_string()),
            _ => {}
        }
        expect = false;
    }
    names
}

/// The dummy argument names of a subprogram statement.
fn dummies(s: &Statement) -> Vec<String> {
    let Some((i, _)) = subprogram_keyword(s) else { return vec![] };
    if s.word(i + 2) != "(" {
        return vec![];
    }
    s.words.iter().skip(i + 3).take_while(|w| w.text != ")").filter(|w| w.text != ",").map(|w| w.text.clone()).collect()
}

/// The merged text and its origins.
struct Builder<'a> {
    input: &'a Input,
    /// The source text of each input origin, shared by its pieces.
    sources: Vec<Arc<str>>,
    text: String,
    origins: Vec<SourceOrigin>,
}

impl<'a> Builder<'a> {
    fn new(input: &'a Input) -> Self {
        let sources = input.origins.iter().map(|o| o.source.clone().unwrap_or_else(|| input.text[o.range.clone()].into())).collect();
        Builder { input, sources, text: String::with_capacity(input.text.len()), origins: vec![] }
    }
    fn map(&mut self, from: Range<usize>, at: usize, len: usize) {
        for (o, source) in self.input.origins.iter().zip(&self.sources) {
            let (start, end) = (o.range.start.max(from.start), o.range.end.min(from.end));
            if start < end || from.is_empty() && (o.range.start..=o.range.end).contains(&from.start) {
                let source = source.clone();
                let range = if from.is_empty() { at..at + len } else { at + start - from.start..at + end - from.start };
                // Extend the previous origin when this text continues it.
                if let Some(last) = self.origins.last_mut() {
                    if last.range.end == range.start && last.name == o.name
                        && last.position(last.range.end) == o.position(start)
                        && last.source.as_ref().is_some_and(|s| Arc::ptr_eq(s, &source)) {
                        last.range.end = range.end;
                        if from.is_empty() {
                            return;
                        }
                        continue;
                    }
                }
                self.origins.push(SourceOrigin { name: o.name.clone(), range, source: Some(source), offset: o.position(start) });
                if from.is_empty() {
                    return;
                }
            }
        }
    }
    fn copy(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let at = self.text.len();
        self.text.push_str(&self.input.text[range.clone()]);
        self.map(range.clone(), at, 0);
        if !self.text.ends_with('\n') {
            self.synth("\n", range.end - 1);
        }
    }
    /// Generated text, reported at source offset `anchor`.
    fn synth(&mut self, text: &str, anchor: usize) {
        let at = self.text.len();
        self.text.push_str(text);
        self.map(anchor..anchor, at, text.len());
    }
}

fn fail(input: &Input, span: Range<usize>, message: String) -> anyhow::Error {
    use codespan_reporting::diagnostic::{Diagnostic, Label};
    use codespan_reporting::term::{termcolor::{ColorChoice, StandardStream}, Config};
    let name = input.origins.first().map_or("<source>", |o| o.name.as_str());
    let file = crate::source_diagnostics::SourceFile::new(name, &input.text, Some(&input.origins));
    let diagnostic = Diagnostic::error().with_message(message).with_labels(vec![Label::primary((), span)]);
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);
    let _ = crate::source_diagnostics::emit_mapped(&mut stderr, &Config::default(), &file, &diagnostic);
    anyhow::anyhow!("compile failed")
}

struct Merger<'a> {
    input: &'a Input,
    stmts: &'a [Statement],
}

impl<'a> Merger<'a> {
    fn range(&self, node: &Node) -> Range<usize> {
        self.stmts[node.open].lines.start..self.stmts[node.close].lines.end
    }
    fn item_range(&self, item: &Item) -> Range<usize> {
        match item {
            Item::Statement(k) => self.stmts[*k].lines.clone(),
            Item::Node(node) => self.range(node),
        }
    }
    fn kind(&self, item: &Item) -> Kind {
        match item {
            Item::Statement(k) => classify(&self.stmts[*k], false),
            Item::Node(node) => node.kind.clone(),
        }
    }
    fn error(&self, k: usize, word: usize, message: String) -> anyhow::Error {
        fail(self.input, self.stmts[k].span(word), message)
    }
    /// The separate interface bodies in the specification part of `unit`.
    fn separate_interfaces(&self, unit: &'a Node) -> Vec<&'a Node> {
        let mut out = vec![];
        for item in items(unit, unit.open + 1, unit.spec_end()) {
            if let Item::Node(block @ Node { kind: Kind::Interface { .. }, .. }) = item {
                out.extend(block.children.iter().filter(|c| matches!(c.kind, Kind::Subprogram { separate: true, .. })));
            }
        }
        out
    }
    /// The separate module procedure bodies after CONTAINS in `unit`.
    fn bodies(&self, unit: &'a Node) -> Vec<&'a Node> {
        let Some(contains) = unit.contains else { return vec![] };
        items(unit, contains + 1, unit.close).into_iter().filter_map(|item| match item {
            Item::Node(n @ Node { kind: Kind::Subprogram { separate: true, .. } | Kind::Procedure(_), .. }) => Some(n),
            _ => None,
        }).collect()
    }
    /// The names `unit` declares, with the statement and word declaring each.
    fn names(&self, unit: &Node) -> Vec<(String, usize, usize)> {
        let mut out = vec![];
        for item in items(unit, unit.open + 1, unit.spec_end()) {
            match item {
                Item::Statement(k) => out.extend(declared(&self.stmts[k]).into_iter().map(|n| (n, k, 0))),
                Item::Node(node) => match &node.kind {
                    Kind::Type(name) => out.push((name.clone(), node.open, 0)),
                    Kind::Interface { generic } => {
                        if let Some(name) = generic.as_ref().filter(|n| !n.is_empty()) {
                            out.push((name.clone(), node.open, 1));
                        }
                        for body in &node.children {
                            if let Kind::Subprogram { name, .. } = &body.kind {
                                out.push((name.clone(), body.open, 0));
                            }
                        }
                    }
                    _ => {}
                },
            }
        }
        if let Some(contains) = unit.contains {
            for item in items(unit, contains + 1, unit.close) {
                if let Item::Node(Node { kind: Kind::Subprogram { name, separate: false, .. }, open, .. }) = item {
                    out.push((name.clone(), *open, 0));
                }
            }
        }
        out
    }
    /// Copy an interface block without its separate interface bodies; a
    /// generic block names their procedures instead.
    fn interface(&self, b: &mut Builder, block: &Node) {
        let Kind::Interface { generic } = &block.kind else { unreachable!() };
        let inner = items(block, block.open + 1, block.close);
        let separate = |item: &Item| matches!(item, Item::Node(Node { kind: Kind::Subprogram { separate: true, .. }, .. }));
        if !inner.iter().any(separate) {
            return b.copy(self.range(block));
        }
        if generic.is_none() && inner.iter().all(separate) {
            return;
        }
        b.copy(self.stmts[block.open].lines.clone());
        for item in &inner {
            match item {
                Item::Node(body @ Node { kind: Kind::Subprogram { name, separate: true, .. }, .. }) => {
                    if generic.is_some() {
                        b.synth(&format!("module procedure {name}\n"), self.stmts[body.open].span(0).start);
                    }
                }
                _ => b.copy(self.item_range(item)),
            }
        }
        b.copy(self.stmts[block.close].lines.clone());
    }
    /// Copy the specification part of `unit`; for a submodule, without the
    /// USE statements (already copied) and IMPLICIT statements.
    fn specification(&self, b: &mut Builder, unit: &Node, submodule: bool) {
        for item in items(unit, unit.open + 1, unit.spec_end()) {
            match (&item, self.kind(&item)) {
                (Item::Statement(_), Kind::Use | Kind::Implicit) if submodule => {}
                (Item::Node(block), Kind::Interface { .. }) => self.interface(b, block),
                _ => b.copy(self.item_range(&item)),
            }
        }
    }
    /// Copy a subprogram statement without its MODULE prefix.
    fn header(&self, b: &mut Builder, k: usize) {
        let at = b.text.len();
        let s = &self.stmts[k];
        b.copy(s.lines.clone());
        if let Some(word) = s.words.iter().take_while(|w| w.text != "function" && w.text != "subroutine").find(|w| w.text == "module") {
            let start = at + word.at - s.lines.start;
            b.text.replace_range(start..start + word.text.len(), &" ".repeat(word.text.len()));
        }
    }
    /// Copy the subprograms after CONTAINS in `unit`, each separate module
    /// procedure body as a module procedure.
    fn subprograms(&self, b: &mut Builder, unit: &Node, interfaces: &HashMap<String, &Node>) {
        let Some(contains) = unit.contains else { return };
        for item in items(unit, contains + 1, unit.close) {
            match item {
                Item::Node(body @ Node { kind: Kind::Subprogram { separate: true, .. }, .. }) => {
                    self.header(b, body.open);
                    b.copy(self.stmts[body.open].lines.end..self.range(body).end);
                }
                Item::Node(body @ Node { kind: Kind::Procedure(name), .. }) => self.procedure(b, body, interfaces[name]),
                _ => b.copy(self.item_range(&item)),
            }
        }
    }
    /// A `MODULE PROCEDURE name` body with the statements of its interface:
    /// their USE statements, then IMPLICIT, then the interface's declarations
    /// and the body's own statements.
    fn procedure(&self, b: &mut Builder, body: &Node, interface: &Node) {
        let Kind::Subprogram { name, function, .. } = &interface.kind else { unreachable!() };
        self.header(b, interface.open);
        let declared = items(interface, interface.open + 1, interface.close);
        let own = items(body, body.open + 1, body.spec_end());
        let kinds = |list: &[Item]| list.iter().map(|i| self.kind(i)).collect::<Vec<_>>();
        let (declared_kinds, own_kinds) = (kinds(&declared), kinds(&own));
        let copy = |b: &mut Builder, list: &[Item], kinds: &[Kind], keep: &dyn Fn(&Kind) -> bool| {
            for (item, kind) in list.iter().zip(kinds) {
                if keep(kind) {
                    b.copy(self.item_range(item));
                }
            }
        };
        copy(b, &declared, &declared_kinds, &|k| *k == Kind::Use);
        copy(b, &own, &own_kinds, &|k| *k == Kind::Use);
        if declared_kinds.contains(&Kind::Implicit) {
            copy(b, &declared, &declared_kinds, &|k| *k == Kind::Implicit);
        } else {
            copy(b, &own, &own_kinds, &|k| *k == Kind::Implicit);
        }
        copy(b, &declared, &declared_kinds, &|k| !matches!(k, Kind::Use | Kind::Implicit | Kind::Import));
        copy(b, &own, &own_kinds, &|k| !matches!(k, Kind::Use | Kind::Implicit));
        if let Some(contains) = body.contains {
            b.copy(self.stmts[contains].lines.start..self.stmts[body.close].lines.start);
        }
        let form = if *function { "function" } else { "subroutine" };
        b.synth(&format!("end {form} {name}\n"), self.stmts[body.close].span(0).start);
    }
    /// Merge `family`, the submodules of `module`, into the module.
    fn module(&self, b: &mut Builder, module: &Node, family: &[&Node]) -> Result<()> {
        let module_name = match &module.kind { Kind::Module(name) => name.clone(), _ => unreachable!() };
        let units: Vec<&Node> = std::iter::once(module).chain(family.iter().copied()).collect();
        let mut interfaces: HashMap<String, &Node> = HashMap::new();
        for unit in &units {
            for interface in self.separate_interfaces(unit) {
                let Kind::Subprogram { name, .. } = &interface.kind else { unreachable!() };
                if interfaces.insert(name.clone(), interface).is_some() {
                    return Err(self.error(interface.open, 0, format!("separate module procedure `{name}` has more than one interface")));
                }
            }
        }
        let mut implemented = HashSet::new();
        for unit in &units {
            for body in self.bodies(unit) {
                let (name, word) = match &body.kind {
                    Kind::Procedure(name) => (name.clone(), 2),
                    Kind::Subprogram { name, .. } => (name.clone(), subprogram_keyword(&self.stmts[body.open]).map_or(0, |(i, _)| i + 1)),
                    _ => unreachable!(),
                };
                let Some(interface) = interfaces.get(&name) else {
                    return Err(self.error(body.open, word, format!("separate module procedure `{name}` has no interface in module `{module_name}` or its submodules")));
                };
                if !implemented.insert(name.clone()) {
                    return Err(self.error(body.open, word, format!("separate module procedure `{name}` has more than one body")));
                }
                if let (Kind::Subprogram { function, .. }, Kind::Subprogram { function: declared, .. }) = (&body.kind, &interface.kind) {
                    let (own, expected) = (dummies(&self.stmts[body.open]), dummies(&self.stmts[interface.open]));
                    if function != declared {
                        let form = |f: &bool| if *f { "a function" } else { "a subroutine" };
                        return Err(self.error(body.open, word, format!("separate module procedure `{name}` is {} but its interface declares {}", form(function), form(declared))));
                    }
                    if own != expected {
                        return Err(self.error(body.open, word, format!("dummy arguments ({}) of separate module procedure `{name}` differ from its interface ({})", own.join(", "), expected.join(", "))));
                    }
                }
            }
        }
        let mut missing: Vec<_> = interfaces.iter().filter(|(name, _)| !implemented.contains(*name)).collect();
        missing.sort_by_key(|(_, node)| node.open);
        if let Some((name, interface)) = missing.first() {
            let word = subprogram_keyword(&self.stmts[interface.open]).map_or(0, |(i, _)| i + 1);
            return Err(self.error(interface.open, word, format!(
                "separate module procedure `{name}` has no body; compile the submodule that defines it together with module `{module_name}` (separate compilation of submodules is not yet supported)")));
        }
        // The submodules' entities become private module entities.
        let mut seen: HashMap<String, String> = self.names(module).into_iter().map(|(n, ..)| (n, format!("module `{module_name}`"))).collect();
        seen.extend(interfaces.keys().map(|n| (n.clone(), format!("module `{module_name}`"))));
        let mut private = vec![];
        for unit in family {
            let Kind::Submodule { name: unit_name, .. } = &unit.kind else { unreachable!() };
            for (name, k, word) in self.names(unit) {
                if interfaces.get(&name).is_some_and(|i| i.open == k) {
                    private.push(name);
                    continue;
                }
                if let Some(owner) = seen.get(&name) {
                    let word = self.stmts[k].words.iter().position(|w| w.text == name).unwrap_or(word);
                    return Err(self.error(k, word, format!(
                        "`{name}` in submodule `{unit_name}` is also declared in {owner}; such names are not yet supported in submodules")));
                }
                seen.insert(name.clone(), format!("submodule `{unit_name}`"));
                private.push(name);
            }
        }
        b.copy(self.stmts[module.open].lines.clone());
        for unit in family {
            for item in items(unit, unit.open + 1, unit.spec_end()) {
                if matches!(item, Item::Statement(_)) && self.kind(&item) == Kind::Use {
                    b.copy(self.item_range(&item));
                }
            }
        }
        self.specification(b, module, false);
        for unit in family {
            self.specification(b, unit, true);
        }
        let anchor = self.stmts[module.open].span(0).start;
        for names in private.chunks(8) {
            b.synth(&format!("private :: {}\n", names.join(", ")), anchor);
        }
        match module.contains {
            Some(contains) => b.copy(self.stmts[contains].lines.clone()),
            None if family.iter().any(|u| u.contains.is_some()) => b.synth("contains\n", self.stmts[module.close].span(0).start),
            None => {}
        }
        for unit in &units {
            self.subprograms(b, unit, &interfaces);
        }
        b.copy(self.stmts[module.close].lines.clone());
        Ok(())
    }
}

/// Merge each submodule of `input` into its ancestor module.
pub(crate) fn merge(input: Input) -> Result<Input> {
    let lower = input.text.to_ascii_lowercase();
    if !lower.contains("submodule") && !lower.contains("module") {
        return Ok(input);
    }
    let stmts = statements(&input.text);
    let submodule = stmts.iter().position(|s| matches!(classify(s, false), Kind::Submodule { .. }));
    let separate = stmts.iter().any(|s| matches!(classify(s, false), Kind::Subprogram { separate: true, .. } | Kind::Procedure(_)));
    if submodule.is_none() && !separate {
        return Ok(input);
    }
    let Some(roots) = tree(&stmts) else {
        return match submodule {
            Some(k) => Err(fail(&input, stmts[k].span(0), "cannot find where the program units around this submodule end".into())),
            None => Ok(input),
        };
    };
    let merger = Merger { input: &input, stmts: &stmts };
    let modules: HashMap<&str, &Node> = roots.iter().filter_map(|n| match &n.kind { Kind::Module(name) => Some((name.as_str(), n)), _ => None }).collect();
    let mut families: HashMap<&str, Vec<&Node>> = HashMap::new();
    let mut known: HashMap<(&str, &str), usize> = HashMap::new();
    for unit in &roots {
        let Kind::Submodule { ancestor, parent, name } = &unit.kind else { continue };
        if !modules.contains_key(ancestor.as_str()) {
            return Err(merger.error(unit.open, 2, format!(
                "submodule `{name}`: its ancestor module `{ancestor}` must be in the same compilation (separate compilation of submodules is not yet supported)")));
        }
        if let Some(parent) = parent {
            if !known.contains_key(&(ancestor.as_str(), parent.as_str())) {
                return Err(merger.error(unit.open, 4, format!("submodule `{name}`: its parent submodule `{ancestor}:{parent}` must come before it in the same compilation")));
            }
        }
        if known.insert((ancestor.as_str(), name.as_str()), unit.open).is_some() {
            return Err(merger.error(unit.open, unit_name_word(&stmts[unit.open]), format!("module `{ancestor}` has more than one submodule named `{name}`")));
        }
        families.entry(ancestor.as_str()).or_default().push(unit);
    }
    let merges = |unit: &Node| match &unit.kind {
        Kind::Module(name) => families.contains_key(name.as_str()) || !merger.separate_interfaces(unit).is_empty() || !merger.bodies(unit).is_empty(),
        _ => false,
    };
    if families.is_empty() && !roots.iter().any(merges) {
        return Ok(input);
    }
    let mut b = Builder::new(&input);
    let mut pos = 0;
    for unit in &roots {
        let range = merger.range(unit);
        b.copy(pos..range.start);
        pos = range.end;
        match &unit.kind {
            Kind::Submodule { .. } => {}
            Kind::Module(name) if merges(unit) => {
                merger.module(&mut b, unit, families.get(name.as_str()).map_or(&[][..], |f| f.as_slice()))?;
            }
            _ => b.copy(range),
        }
    }
    b.copy(pos..input.text.len());
    Ok(Input { text: b.text, origins: b.origins })
}

fn unit_name_word(s: &Statement) -> usize {
    s.words.len().saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merged(text: &str) -> String {
        let input = Input {
            text: text.into(),
            origins: vec![SourceOrigin { name: "t.f90".into(), range: 0..text.len(), source: None, offset: 0 }],
        };
        let out = merge(input).unwrap();
        // Every byte of the merged text maps to the original source.
        let mut covered = 0;
        for o in &out.origins {
            assert_eq!(o.range.start, covered, "{:?}", out.origins);
            covered = o.range.end;
        }
        assert_eq!(covered, out.text.len());
        out.text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn statements_join_continuations_and_split_semicolons() {
        let s = statements("x = 1; y = 'a;b' ! c\nz = f(1, &\n  ! note\n  & 2)\n");
        let words: Vec<Vec<&str>> = s.iter().map(|s| s.words.iter().map(|w| w.text.as_str()).collect()).collect();
        assert_eq!(words, [vec!["x", "=", "1"], vec!["y", "=", "'a;b'"], vec!["z", "=", "f", "(", "1", ",", "2", ")"]]);
        assert_eq!(s.last().unwrap().lines.end, 48);
    }

    #[test]
    fn procedure_bodies_take_their_interfaces() {
        let text = "module m\nimplicit none\ninterface\nmodule function f(x) result(y)\nreal, intent(in) :: x\nreal :: y\nend function\nend interface\nend module m\nsubmodule (m) s\nreal :: scale = 2\ncontains\nmodule procedure f\ny = scale * x\nend procedure\nend submodule s\n";
        assert_eq!(merged(text), "module m\nimplicit none\nreal :: scale = 2\nprivate :: scale\ncontains\nfunction f(x) result(y)\nreal, intent(in) :: x\nreal :: y\ny = scale * x\nend function f\nend module m");
    }

    #[test]
    fn generic_blocks_name_their_separate_procedures() {
        let text = "module m\ninterface g\nmodule subroutine a(x)\ninteger :: x\nend subroutine\nend interface\ncontains\nmodule subroutine a(x)\ninteger :: x\nx = 1\nend subroutine\nend module\n";
        assert_eq!(merged(text), "module m\ninterface g\nmodule procedure a\nend interface\ncontains\nsubroutine a(x)\ninteger :: x\nx = 1\nend subroutine\nend module");
    }

    #[test]
    fn text_without_submodules_is_unchanged() {
        let text = "module m\ninterface g\nmodule procedure a\nend interface\ncontains\nsubroutine a()\nend\nend module\n";
        let input = Input { text: text.into(), origins: vec![] };
        assert_eq!(merge(input).unwrap().text, text);
    }
}

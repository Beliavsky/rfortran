// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Conditional compilation with C preprocessor directives, as gfortran applies
//! to sources with upper-case extensions such as `.F` and `.F90`, or with
//! `-cpp`: `#if`, `#ifdef`, `#ifndef`, `#elif`, `#else`, `#endif`, `#define`,
//! `#undef` and `#error`. Conditions are C integer expressions with
//! `defined`, in which a name that is not defined is 0.
//!
//! Directive lines and the lines that conditions exclude become blank, so the
//! remaining lines keep their line numbers. Names are not replaced in Fortran
//! lines: a line that uses a name defined with a value or in the source is an
//! error rather than being compiled with the name unreplaced.
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

/// Extensions of the sources that gfortran preprocesses.
const PREPROCESSED_EXTENSIONS: &[&str] = &["F", "FOR", "FTN", "FPP", "fpp", "F77", "F90", "F95", "F03", "F08"];

/// Whether a source with this file name is preprocessed by default.
pub fn by_extension(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| PREPROCESSED_EXTENSIONS.contains(&e))
}

/// The definitions of `-D NAME` and `-D NAME=VALUE` options. A name without a
/// value is 1, as in cpp.
pub fn definitions(options: &[String]) -> Result<HashMap<String, Option<String>>> {
    let mut out = HashMap::new();
    for option in options {
        let (name, value) = match option.split_once('=') {
            Some((name, value)) => (name, value.to_string()),
            None => (option.as_str(), "1".to_string()),
        };
        if !identifier(name) {
            bail!("invalid -D name `{name}`");
        }
        out.insert(name.to_string(), Some(value));
    }
    Ok(out)
}

fn identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// One open `#if` group.
struct Group {
    /// Whether the lines of the enclosing group are kept.
    outer: bool,
    /// Whether a branch of this group has been taken.
    taken: bool,
    /// Whether the current branch's lines are kept.
    active: bool,
    /// Whether `#else` has been seen.
    after_else: bool,
}

/// Preprocess `text`. `fixed` says whether it is fixed source form, whose
/// comment lines are not checked for defined names.
pub fn preprocess(text: &str, defined: &HashMap<String, Option<String>>, fixed: bool) -> Result<String> {
    let mut defined = defined.clone();
    let mut groups: Vec<Group> = vec![];
    let mut out = String::with_capacity(text.len());
    for (index, line) in text.split_inclusive('\n').enumerate() {
        let number = index + 1;
        let ending = &line[line.trim_end_matches(['\r', '\n']).len()..];
        let content = line.trim_end_matches(['\r', '\n']);
        let active = groups.last().is_none_or(|g| g.active);
        let Some(directive) = content.trim_start().strip_prefix('#') else {
            if active {
                if let Some(name) = uses_definition(content, &defined, fixed) {
                    bail!("line {number}: `{name}` is a preprocessor name; replacing names in Fortran lines is not supported");
                }
                out.push_str(line);
            } else {
                out.push_str(ending);
            }
            continue;
        };
        out.push_str(ending);
        let directive = directive.trim_start();
        let word_end = directive.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(directive.len());
        let (word, rest) = directive.split_at(word_end);
        let rest = strip_comment(rest).trim();
        let context = || format!("line {number}: #{word}");
        match word {
            "if" | "ifdef" | "ifndef" => {
                let value = if !active {
                    false
                } else if word == "if" {
                    evaluate(rest, &defined).with_context(context)? != 0
                } else {
                    if !identifier(rest) {
                        bail!("line {number}: #{word} requires a name");
                    }
                    defined.contains_key(rest) == (word == "ifdef")
                };
                groups.push(Group { outer: active, taken: value, active: value, after_else: false });
            }
            "elif" => {
                let group = groups.last_mut().with_context(|| format!("line {number}: #elif without #if"))?;
                if group.after_else {
                    bail!("line {number}: #elif after #else");
                }
                group.active = if group.outer && !group.taken {
                    evaluate(rest, &defined).with_context(context)? != 0
                } else {
                    false
                };
                group.taken |= group.active;
            }
            "else" => {
                let group = groups.last_mut().with_context(|| format!("line {number}: #else without #if"))?;
                if group.after_else {
                    bail!("line {number}: second #else");
                }
                group.after_else = true;
                group.active = group.outer && !group.taken;
                group.taken = true;
            }
            "endif" => {
                groups.pop().with_context(|| format!("line {number}: #endif without #if"))?;
            }
            _ if !active => {}
            "define" => {
                let end = rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(rest.len());
                let (name, value) = rest.split_at(end);
                if !identifier(name) {
                    bail!("line {number}: #define requires a name");
                }
                if value.starts_with('(') {
                    bail!("line {number}: function-like macros are not supported");
                }
                let value = value.trim();
                defined.insert(name.to_string(), Some(if value.is_empty() { String::new() } else { value.to_string() }));
            }
            "undef" => {
                defined.remove(rest);
            }
            "error" => bail!("line {number}: #error {rest}"),
            // A null directive.
            "" => {}
            _ => bail!("line {number}: the #{word} directive is not supported"),
        }
    }
    if !groups.is_empty() {
        bail!("#if without #endif");
    }
    Ok(out)
}

/// The text before a C comment.
fn strip_comment(s: &str) -> &str {
    let end = [s.find("/*"), s.find("//")].into_iter().flatten().min().unwrap_or(s.len());
    &s[..end]
}

/// A name of `defined` that `line` uses outside character constants and
/// comments. Names from -D without an explicit value are reported too, since
/// cpp would replace them by 1.
fn uses_definition<'a>(line: &str, defined: &'a HashMap<String, Option<String>>, fixed: bool) -> Option<&'a str> {
    if defined.is_empty() || fixed && line.starts_with(['c', 'C', '*', 'd', 'D']) {
        return None;
    }
    let bytes = line.as_bytes();
    let mut quote = None;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == b'\'' || c == b'"' => quote = Some(c),
            None if c == b'!' => break,
            None if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if let Some((name, _)) = defined.get_key_value(&line[start..i]) {
                    return Some(name);
                }
                continue;
            }
            None => {}
        }
        i += 1;
    }
    None
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(i64),
    Name(String),
    Op(&'static str),
}

fn tokens(s: &str) -> Result<Vec<Token>> {
    const OPS: &[&str] = &["&&", "||", "==", "!=", "<=", ">=", "<<", ">>", "(", ")", "!", "~", "*", "/", "%", "+", "-",
                           "<", ">", "&", "^", "|", "?", ":"];
    let mut out = vec![];
    let mut rest = s.trim_start();
    while !rest.is_empty() {
        let c = rest.chars().next().unwrap();
        if c.is_ascii_digit() {
            let end = rest.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(rest.len());
            let digits = rest[..end].trim_end_matches(['u', 'U', 'l', 'L']);
            let value = if let Some(hex) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
                i64::from_str_radix(hex, 16)
            } else if digits.len() > 1 && digits.starts_with('0') {
                i64::from_str_radix(&digits[1..], 8)
            } else {
                digits.parse()
            };
            out.push(Token::Number(value.with_context(|| format!("invalid number `{}`", &rest[..end]))?));
            rest = &rest[end..];
        } else if c.is_ascii_alphabetic() || c == '_' {
            let end = rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(rest.len());
            out.push(Token::Name(rest[..end].to_string()));
            rest = &rest[end..];
        } else if let Some(op) = OPS.iter().find(|op| rest.starts_with(**op)) {
            out.push(Token::Op(op));
            rest = &rest[op.len()..];
        } else {
            bail!("unexpected `{c}` in a condition");
        }
        rest = rest.trim_start();
    }
    Ok(out)
}

/// The value of the condition `s`.
fn evaluate(s: &str, defined: &HashMap<String, Option<String>>) -> Result<i64> {
    // `defined NAME` and `defined(NAME)` first, then the values of names.
    let raw = tokens(s)?;
    let mut expanded = vec![];
    let mut i = 0;
    while i < raw.len() {
        match &raw[i] {
            Token::Name(n) if n == "defined" => {
                let (name, next) = match (raw.get(i + 1), raw.get(i + 2), raw.get(i + 3)) {
                    (Some(Token::Op("(")), Some(Token::Name(name)), Some(Token::Op(")"))) => (name, i + 4),
                    (Some(Token::Name(name)), _, _) => (name, i + 2),
                    _ => bail!("`defined` requires a name"),
                };
                expanded.push(Token::Number(defined.contains_key(name) as i64));
                i = next;
                continue;
            }
            Token::Name(n) => match defined.get(n) {
                Some(Some(value)) if !value.is_empty() => {
                    let mut seen = vec![n.clone()];
                    expanded.extend(expand(value, defined, &mut seen)?);
                }
                Some(_) => bail!("`{n}` is defined without a value"),
                None => expanded.push(Token::Number(0)),
            },
            t => expanded.push(t.clone()),
        }
        i += 1;
    }
    if expanded.is_empty() {
        bail!("missing condition");
    }
    let mut parser = Parser { tokens: &expanded, at: 0 };
    let value = parser.conditional()?;
    if parser.at != expanded.len() {
        bail!("unexpected tokens after the condition");
    }
    Ok(value)
}

/// The tokens of a name's value, with the names in it replaced in turn.
fn expand(value: &str, defined: &HashMap<String, Option<String>>, seen: &mut Vec<String>) -> Result<Vec<Token>> {
    let mut out = vec![];
    for t in tokens(value)? {
        match t {
            Token::Name(n) if !seen.contains(&n) => match defined.get(&n) {
                Some(Some(v)) if !v.is_empty() => {
                    seen.push(n);
                    out.extend(expand(v, defined, seen)?);
                    seen.pop();
                }
                _ => out.push(Token::Number(0)),
            },
            Token::Name(_) => out.push(Token::Number(0)),
            t => out.push(t),
        }
    }
    Ok(out)
}

struct Parser<'a> {
    tokens: &'a [Token],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn eat(&mut self, op: &str) -> bool {
        if matches!(self.peek(), Some(Token::Op(o)) if *o == op) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn conditional(&mut self) -> Result<i64> {
        let c = self.binary(0)?;
        if !self.eat("?") {
            return Ok(c);
        }
        let a = self.conditional()?;
        if !self.eat(":") {
            bail!("`?` without `:`");
        }
        let b = self.conditional()?;
        Ok(if c != 0 { a } else { b })
    }

    /// Binary operators at precedence `level` and above.
    fn binary(&mut self, level: usize) -> Result<i64> {
        const LEVELS: &[&[&str]] = &[&["||"], &["&&"], &["|"], &["^"], &["&"], &["==", "!="], &["<", "<=", ">", ">="],
                                     &["<<", ">>"], &["+", "-"], &["*", "/", "%"]];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        loop {
            let Some(Token::Op(op)) = self.peek().cloned() else { break };
            if !LEVELS[level].contains(&op) {
                break;
            }
            self.at += 1;
            let right = self.binary(level + 1)?;
            left = match op {
                "||" => (left != 0 || right != 0) as i64,
                "&&" => (left != 0 && right != 0) as i64,
                "|" => left | right,
                "^" => left ^ right,
                "&" => left & right,
                "==" => (left == right) as i64,
                "!=" => (left != right) as i64,
                "<" => (left < right) as i64,
                "<=" => (left <= right) as i64,
                ">" => (left > right) as i64,
                ">=" => (left >= right) as i64,
                "<<" => left.wrapping_shl(right as u32),
                ">>" => left.wrapping_shr(right as u32),
                "+" => left.wrapping_add(right),
                "-" => left.wrapping_sub(right),
                "*" => left.wrapping_mul(right),
                "/" | "%" if right == 0 => bail!("division by zero in a condition"),
                "/" => left.wrapping_div(right),
                _ => left.wrapping_rem(right),
            };
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<i64> {
        if self.eat("!") {
            return Ok((self.unary()? == 0) as i64);
        }
        if self.eat("~") {
            return Ok(!self.unary()?);
        }
        if self.eat("-") {
            return Ok(self.unary()?.wrapping_neg());
        }
        if self.eat("+") {
            return self.unary();
        }
        if self.eat("(") {
            let value = self.conditional()?;
            if !self.eat(")") {
                bail!("missing `)` in a condition");
            }
            return Ok(value);
        }
        match self.peek().cloned() {
            Some(Token::Number(n)) => {
                self.at += 1;
                Ok(n)
            }
            _ => bail!("incomplete condition"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, defines: &[&str]) -> Result<String> {
        let d = definitions(&defines.iter().map(|s| s.to_string()).collect::<Vec<_>>())?;
        preprocess(text, &d, false)
    }

    #[test]
    fn openmp_blocks_of_lapack() {
        let text = "a\n#if defined(_OPENMP)\nb\n#endif\n#if defined(_OPENMP) && _OPENMP >= 201307\nc\n#else\nd\n#endif\ne\n";
        assert_eq!(run(text, &[]).unwrap(), "a\n\n\n\n\n\n\nd\n\ne\n");
        assert_eq!(run(text, &["_OPENMP=201511"]).unwrap(), "a\n\nb\n\n\nc\n\n\n\ne\n");
        assert_eq!(run(text, &["_OPENMP=201107"]).unwrap(), "a\n\nb\n\n\n\n\nd\n\ne\n");
    }

    #[test]
    fn nesting_elif_and_define() {
        let text = "#define LEVEL 2\n#ifdef MISSING\nx\n#elif LEVEL == 1\ny\n#elif (LEVEL << 1) == 4 ? 1 : 0\nz\n#if 0\nw\n#else\nv\n#endif\n#else\nu\n#endif\n#ifndef LEVEL\nt\n#endif\n";
        assert_eq!(run(text, &[]).unwrap(), "\n\n\n\n\n\nz\n\n\n\nv\n\n\n\n\n\n\n\n");
        assert!(run("#if 1\nx\n", &[]).is_err());
        assert!(run("#endif\n", &[]).is_err());
        assert!(run("#include \"a.h\"\n", &[]).is_err());
        assert!(run("#if 0\n#include \"a.h\"\n#endif\n", &[]).is_ok());
        assert!(run("#error stop\n", &[]).is_err());
    }

    #[test]
    fn defined_names_in_fortran_lines_are_errors() {
        assert!(run("x = N + 1\n", &["N=3"]).is_err());
        assert!(run("x = 'N' + 1 ! N\n", &["N=3"]).is_ok());
        assert!(run("#define N 3\n#undef N\nx = N\n", &[]).is_ok());
        // CRLF line endings are kept.
        assert_eq!(run("a\r\n#if 0\r\nb\r\n#endif\r\n", &[]).unwrap(), "a\r\n\r\n\r\n\r\n");
    }
}

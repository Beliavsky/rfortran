// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Fortran INCLUDE lines, expanded after source-form conversion and CPP
//! conditionals. Included text inherits source form, but is not preprocessed.
use crate::{cli::Cli, source_diagnostics::SourceOrigin};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub struct Input {
    pub text: String,
    pub origins: Vec<SourceOrigin>,
}

fn filename(line: &str) -> Result<Option<String>> {
    let line = line.trim_start();
    if !line
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("include"))
    {
        return Ok(None);
    }
    let rest = &line[7..];
    // INCLUDE is not reserved: it may name a variable, component, array,
    // or construct. Preserve ordinary statements even with intervening blanks.
    if rest.trim_start().starts_with(['=', '(', '%', ':']) {
        return Ok(None);
    }
    if !rest.starts_with(|c: char| c.is_whitespace() || c == '\'' || c == '"') {
        return Ok(None);
    }
    let mut chars = rest.trim_start().chars().peekable();
    let Some(quote @ ('\'' | '"')) = chars.next() else {
        bail!("INCLUDE requires a quoted filename");
    };
    let mut name = String::new();
    while let Some(c) = chars.next() {
        if c == quote {
            if chars.peek() == Some(&quote) {
                chars.next();
                name.push(c);
                continue;
            }
            let tail = chars.collect::<String>();
            if !tail.trim_start().is_empty() && !tail.trim_start().starts_with('!') {
                bail!("INCLUDE must occupy a complete source line");
            }
            if name.is_empty() {
                bail!("INCLUDE filename is empty");
            }
            return Ok(Some(name));
        }
        name.push(c);
    }
    bail!("unterminated INCLUDE filename")
}

// Do not interpret text in a continued statement or character literal as an
// INCLUDE line. A line's final ampersand precedes any trailing comment.
fn continuation(line: &str, mut quote: Option<char>) -> (bool, Option<char>) {
    let mut chars = line.trim_end_matches(['\r', '\n']).chars().peekable();
    let mut last = None;
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if c == q {
                if chars.peek() == Some(&q) {
                    chars.next();
                } else {
                    quote = None;
                }
            }
        } else if c == '!' {
            break;
        } else if c == '\'' || c == '"' {
            quote = Some(c);
        }
        if !c.is_whitespace() {
            last = Some(c);
        }
    }
    let continued = last == Some('&');
    (continued, if continued { quote } else { None })
}

fn find(name: &str, parent: &Path, cli: &Cli) -> Result<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        if path.is_file() {
            return Ok(path.to_owned());
        }
    } else {
        for directory in std::iter::once(parent.parent().unwrap_or(Path::new(".")))
            .chain(std::iter::once(Path::new(".")))
            .chain(cli.module_paths.iter().map(PathBuf::as_path))
        {
            let candidate = directory.join(path);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    bail!("INCLUDE file `{name}` not found (searched source directory, current directory and -I directories)")
}

fn expand(
    path: &Path,
    cli: &Cli,
    fixed: bool,
    cpp: bool,
    stack: &mut Vec<PathBuf>,
) -> Result<Input> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("reading {}", path.display()))?;
    if stack.iter().any(|p| {
        p == &canonical
            || cfg!(windows)
                && p.to_string_lossy()
                    .eq_ignore_ascii_case(&canonical.to_string_lossy())
    }) {
        let chain = stack
            .iter()
            .chain(std::iter::once(&canonical))
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(" -> ");
        bail!("recursive INCLUDE cycle: {chain}");
    }
    if stack.len() >= 128 {
        bail!("INCLUDE nesting exceeds 128 files at {}", path.display());
    }
    stack.push(canonical);
    let source = crate::utils::read_source_unit(path, cli, fixed, cpp)?;
    let mut result = Input {
        text: String::new(),
        origins: vec![],
    };
    let mut start = 0;
    let mut offset = 0;
    let mut continued = false;
    let mut quote = None;
    let mut has_include = false;
    fn chunk(out: &mut Input, source: &str, name: &str, start: usize, end: usize) {
        if start == end {
            return;
        }
        let at = out.text.len();
        out.text.push_str(&source[start..end]);
        out.origins.push(SourceOrigin {
            name: name.into(),
            range: at..out.text.len(),
            source: Some(source.into()),
            offset: start,
        });
    }
    let name = path.display().to_string();
    for (index, line) in source.split_inclusive('\n').enumerate() {
        // Blank and comment lines may intervene in a continued statement.
        let comment = line.trim_start().is_empty() || line.trim_start().starts_with('!');
        if !continued && !comment {
            if let Some(included) =
                filename(line).with_context(|| format!("{name}:{}", index + 1))?
            {
                has_include = true;
                chunk(&mut result, &source, &name, start, offset);
                let child = find(&included, path, cli)
                    .and_then(|p| expand(&p, cli, fixed, false, stack))
                    .with_context(|| format!("{name}:{}: INCLUDE `{included}`", index + 1))?;
                let at = result.text.len();
                result.text.push_str(&child.text);
                result
                    .origins
                    .extend(child.origins.into_iter().map(|mut o| {
                        o.range = o.range.start + at..o.range.end + at;
                        o
                    }));
                // Separate the included final record from the following one.
                if !result.text.ends_with('\n') {
                    result.text.push('\n');
                }
                start = offset + line.len();
            }
        }
        if !comment {
            (continued, quote) = continuation(line, quote);
        }
        offset += line.len();
    }
    stack.pop();
    if has_include {
        chunk(&mut result, &source, &name, start, source.len());
    } else {
        result.text = source;
        result.origins = vec![SourceOrigin {
            name,
            range: 0..result.text.len(),
            source: None,
            offset: 0,
        }];
    }
    Ok(result)
}

pub fn read(path: &Path, cli: &Cli) -> Result<Input> {
    expand(
        path,
        cli,
        cli.is_fixed_form(path),
        !cli.nocpp && (cli.cpp || crate::preprocess::by_extension(path)),
        &mut vec![],
    )
}

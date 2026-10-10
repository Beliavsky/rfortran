// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Supported explicit output descriptors. Reject unsupported formats explicitly.
use anyhow::{bail, Result};
#[derive(Clone, Debug)]
pub struct Field {
    pub code: char,
    pub width: usize,
    pub precision: usize,
    pub literal: Option<String>,
}
pub fn parse(format: &str, count: usize) -> Result<Vec<Field>> {
    let format = format.trim();
    let Some(inner) = format.strip_prefix('(').and_then(|s| s.strip_suffix(')')) else {
        bail!("output format must be parenthesized");
    };
    let mut fields = vec![];
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut chars = inner.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if let Some(q) = quote {
            if c == q {
                if chars.peek().is_some_and(|(_, next)| *next == q) {
                    chars.next();
                } else {
                    quote = None;
                }
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
        } else if c == ',' {
            parts.push(&inner[start..i]);
            start = i + 1;
        }
    }
    if quote.is_some() {
        bail!("unterminated literal in output format");
    }
    parts.push(&inner[start..]);
    for item in parts {
        let item = item.trim();
        if item.starts_with(['\'', '"']) {
            let quote = item.chars().next().unwrap();
            if item.len() < 2 || !item.ends_with(quote) {
                bail!("invalid literal output descriptor");
            }
            let literal =
                item[1..item.len() - 1].replace(&format!("{quote}{quote}"), &quote.to_string());
            fields.push(Field {
                code: 'a',
                width: 0,
                precision: 0,
                literal: Some(literal),
            });
            continue;
        }
        let Some(pos) = item.find(|c: char| c.is_ascii_alphabetic()) else {
            bail!("invalid output descriptor");
        };
        let repeat: usize = if pos == 0 { 1 } else { item[..pos].parse()? };
        let code = (item.as_bytes()[pos] as char).to_ascii_lowercase();
        if !matches!(code, 'a' | 'i' | 'f' | 'l') {
            bail!("unsupported output descriptor `{code}`");
        }
        let tail = &item[pos + 1..];
        let (width, precision) = if code == 'f' || (code == 'i' && tail.contains('.')) {
            let Some((w, p)) = tail.split_once('.') else {
                bail!("F descriptor requires width and precision");
            };
            (w.parse()?, p.parse()?)
        } else {
            (
                if tail.is_empty() && code == 'a' {
                    0
                } else {
                    tail.parse()?
                },
                if code == 'i' { 1 } else { 0 },
            )
        };
        if repeat == 0 || repeat > 10000 || width > 256 || precision > 256 {
            bail!("output descriptor exceeds supported limits");
        }
        fields.extend((0..repeat).map(|_| Field {
            code,
            width,
            precision,
            literal: None,
        }));
    }
    if fields.iter().filter(|f| f.literal.is_none()).count() != count {
        bail!("format descriptor count must match the output list; format reversion is not yet supported");
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    #[test]
    fn validates_explicit_formats() {
        let fields = super::parse("(i3,6f11.4)", 7).unwrap();
        assert_eq!(fields[1].width, 11);
        assert_eq!(fields[6].precision, 4);
        let fields = super::parse("(i0,\"Case,Upper:\",i2.2)", 2).unwrap();
        assert_eq!(fields[1].literal.as_deref(), Some("Case,Upper:"));
        assert_eq!(fields[2].precision, 2);
        let fields = super::parse("('isn''t',i3.0)", 1).unwrap();
        assert_eq!(fields[0].literal.as_deref(), Some("isn't"));
        assert_eq!(fields[1].precision, 0);
        for (fmt, n) in [
            ("i3", 1),
            ("(f11)", 1),
            ("(i3)", 2),
            ("(e12.4)", 1),
            ("(999999i1)", 1),
        ] {
            assert!(super::parse(fmt, n).is_err());
        }
    }
}

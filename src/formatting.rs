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
    let mut parser = crate::format_spec::GroupParser::new(format.as_bytes(),count).map_err(anyhow::Error::msg)?;
    let mut fields = Vec::new();
    let mut items=0;
    let mut finished=false;
    while let Some(d) = parser.next().map_err(anyhow::Error::msg)? {
        let literal = match d.code {
            b'\'' | b'"' => {
                let quote = d.code as char;
                Some(std::str::from_utf8(d.literal)?.replace(&format!("{quote}{quote}"), &quote.to_string()))
            }
            b'x' => Some(" ".into()),
            b'/' => Some("\n".into()),
            _ => None,
        };
        for _ in 0..d.repeat {
            if finished {continue;}
            if literal.is_none() {
                if items==count {finished=true;continue;}
                items+=1;
            }
            fields.push(Field { code: d.code as char, width: d.width, precision: d.precision, literal: literal.clone() });
        }
    }
    if fields.iter().filter(|f| f.literal.is_none()).count() != count {
        bail!("format descriptor count must match the output list; format reversion is not yet supported");
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    #[test]
    fn stops_at_next_data_descriptor_after_last_item() {
        let fields=super::parse("(100(a,1x))",2).unwrap();
        assert_eq!(fields.len(),4);
        assert_eq!(super::parse("(100(a,1x))",0).unwrap().len(),0);
        let fields=super::parse("(i0,'!',i0)",1).unwrap();
        assert_eq!(fields.len(),2);
        assert_eq!(fields[1].literal.as_deref(),Some("!"));
        assert!(super::parse("(2a)",3).is_err());
    }
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

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Compile-time checks of explicit output formats. Rendering is shared with
//! the native runtime in `format_spec`.
use anyhow::Result;
#[derive(Clone, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct Field {
    pub code: char,
    pub width: usize,
    pub precision: usize,
    pub literal: Option<String>,
}
/// The expanded descriptors written for `count` items, including record
/// breaks from format reversion. Fails for an invalid or unsupported format.
pub fn parse(format: &str, count: usize) -> Result<Vec<Field>> {
    let mut parser = crate::format_spec::GroupParser::new(format.as_bytes(),count).map_err(anyhow::Error::msg)?;
    let mut fields = Vec::new();
    while let Some(d) = parser.next().map_err(anyhow::Error::msg)? {
        let literal = match d.code {
            b'\'' | b'"' => {
                let quote = d.code as char;
                Some(std::str::from_utf8(d.literal)?.replace(&format!("{quote}{quote}"), &quote.to_string()))
            }
            b'/' | crate::format_spec::REVERSION => Some("\n".into()),
            code if !crate::format_spec::is_data(code) => Some(String::new()),
            _ => None,
        };
        for _ in 0..d.repeat {
            fields.push(Field { code: d.code as char, width: d.width, precision: d.precision, literal: literal.clone() });
        }
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
    }
    #[test]
    fn reverts_to_the_last_top_level_group() {
        let data=|fields:&[super::Field]|fields.iter().filter(|f|f.literal.is_none()).count();
        let fields=super::parse("(2a)",3).unwrap();
        assert_eq!(data(&fields),3);
        assert_eq!(fields[2].literal.as_deref(),Some("\n"));
        let fields=super::parse("(i2,(i3,'|'))",4).unwrap();
        let codes:String=fields.iter().map(|f|if f.literal.as_deref()==Some("\n") {'/'} else {f.code}).collect();
        assert_eq!(codes,"ii'/i'/i'");
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
        for fmt in ["(e12.4)", "(es10.3e3)", "(1pe12.4)", "(sp,i3)", "(t5,a)", "(b8.8)", "(g12.4)"] {
            assert!(super::parse(fmt, 1).is_ok(), "{fmt}");
        }
        for (fmt, n) in [
            ("i3", 1),
            ("(f11)", 1),
            ("('no data')", 1),
            ("(999999i1)", 1),
            ("(g12)", 1),
            ("(rn,f5.1)", 1),
        ] {
            assert!(super::parse(fmt, n).is_err(), "{fmt}");
        }
    }
}

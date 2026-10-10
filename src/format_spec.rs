// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Allocation-free format parser shared with the standalone native runtime.
#[derive(Clone, Copy, Debug)]
pub struct Descriptor<'a> {
    pub code: u8,
    pub width: usize,
    pub precision: usize,
    pub repeat: usize,
    pub literal: &'a [u8],
}
pub struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Parser<'a> {
    pub fn new(mut bytes: &'a [u8]) -> Result<Self, &'static str> {
        if !bytes.is_ascii() || bytes.contains(&0) {
            return Err("format requires non-NUL ASCII characters");
        }
        while bytes.first().is_some_and(u8::is_ascii_whitespace) {
            bytes = &bytes[1..];
        }
        while bytes.last().is_some_and(u8::is_ascii_whitespace) {
            bytes = &bytes[..bytes.len() - 1];
        }
        if bytes.first() != Some(&b'(') || bytes.last() != Some(&b')') {
            return Err("output format must be parenthesized");
        }
        Ok(Self {
            bytes: &bytes[1..bytes.len() - 1],
            pos: 0,
        })
    }
    fn spaces(&mut self) {
        while self
            .bytes
            .get(self.pos)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.pos += 1;
        }
    }
    fn number(&mut self) -> Result<Option<usize>, &'static str> {
        let start = self.pos;
        let mut n = 0usize;
        while let Some(c @ b'0'..=b'9') = self.bytes.get(self.pos) {
            n = n
                .checked_mul(10)
                .and_then(|n| n.checked_add((c - b'0') as usize))
                .ok_or("output descriptor exceeds supported limits")?;
            self.pos += 1;
        }
        Ok(if self.pos == start { None } else { Some(n) })
    }
    pub fn next(&mut self) -> Result<Option<Descriptor<'a>>, &'static str> {
        self.spaces();
        if self.bytes.get(self.pos) == Some(&b',') {
            self.pos += 1;
            self.spaces();
        }
        if self.pos == self.bytes.len() {
            return Ok(None);
        }
        let mut d = Descriptor {
            code: 0,
            width: 0,
            precision: 0,
            repeat: 1,
            literal: &[],
        };
        let c = self.bytes[self.pos];
        if c == b'\'' || c == b'"' {
            self.pos += 1;
            let start = self.pos;
            loop {
                let Some(&ch) = self.bytes.get(self.pos) else {
                    return Err("unterminated literal in output format");
                };
                self.pos += 1;
                if ch == c {
                    if self.bytes.get(self.pos) == Some(&c) {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
            }
            d.code = c;
            d.literal = &self.bytes[start..self.pos - 1];
        } else {
            d.repeat = self.number()?.unwrap_or(1);
            let Some(&code) = self.bytes.get(self.pos) else {
                return Err("invalid output descriptor");
            };
            d.code = code.to_ascii_lowercase();
            self.pos += 1;
            if !matches!(d.code, b'a' | b'i' | b'f' | b'l' | b'x' | b'/') {
                return Err("unsupported output descriptor");
            }
            if !matches!(d.code, b'x' | b'/') {
                let width = self.number()?;
                if width.is_none() && d.code != b'a' {
                    return Err("output descriptor requires a width");
                }
                d.width = width.unwrap_or(0);
                d.precision = if d.code == b'i' { 1 } else { 0 };
                if self.bytes.get(self.pos) == Some(&b'.') {
                    if !matches!(d.code, b'i' | b'f') {
                        return Err("invalid output descriptor precision");
                    }
                    self.pos += 1;
                    d.precision = self
                        .number()?
                        .ok_or("output descriptor requires a precision")?;
                } else if d.code == b'f' {
                    return Err("F descriptor requires width and precision");
                }
            }
        }
        if d.repeat == 0 || d.repeat > 10000 || d.width > 256 || d.precision > 256 {
            return Err("output descriptor exceeds supported limits");
        }
        self.spaces();
        if let Some(c) = self.bytes.get(self.pos) {
            if *c != b',' && *c != b'/' && d.code != b'/' {
                return Err("expected comma between output descriptors");
            }
        }
        Ok(Some(d))
    }
}

pub fn matches_type(code: u8, tag: u64) -> bool {
    matches!((code, tag), (b'i', 1) | (b'f', 2) | (b'l', 3) | (b'a', 4))
}

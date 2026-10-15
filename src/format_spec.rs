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
#[derive(Clone, Copy)]
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
            if !matches!(d.code, b'a' | b'i' | b'f' | b'g' | b'l' | b'x' | b'/' | b':') {
                return Err("unsupported output descriptor");
            }
            if !matches!(d.code, b'x' | b'/' | b':') {
                let width = self.number()?;
                if width.is_none() && d.code != b'a' {
                    return Err("output descriptor requires a width");
                }
                d.width = width.unwrap_or(0);
                d.precision = if d.code == b'i' { 1 } else if d.code==b'g' {17} else { 0 };
                if self.bytes.get(self.pos) == Some(&b'.') {
                    if !matches!(d.code, b'i' | b'f' | b'g') {
                        return Err("invalid output descriptor precision");
                    }
                    self.pos += 1;
                    d.precision = self
                        .number()?
                        .ok_or("output descriptor requires a precision")?;
                } else if d.code == b'f' {
                    return Err("F descriptor requires width and precision");
                }
                if d.code==b'g' && (d.width!=0 || d.precision==0) {return Err("only REAL G0 and G0.d output are currently supported");}
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
    matches!((code, tag), (b'i', 1) | (b'f', 2) | (b'g', 2) | (b'l', 3) | (b'a', 4))
}

#[derive(Clone, Copy)]
struct Frame<'a> {
    parser: Parser<'a>,
    remaining: usize,
    unlimited: bool,
}

/// Expand finite nested groups and a final unlimited-repeat group without
/// allocating. Unlimited repetition is bounded by the actual output list.
pub struct GroupParser<'a> {
    frames: [Frame<'a>; 32],
    depth: usize,
    count: usize,
    consumed: usize,
    operations: usize,
}
impl<'a> Parser<'a> {
    fn separator(&mut self) {
        self.spaces();
        if self.bytes.get(self.pos) == Some(&b',') {
            self.pos += 1;
            self.spaces();
        }
    }
    fn group(&mut self) -> Result<Option<(&'a [u8], usize, bool)>, &'static str> {
        let saved = self.pos;
        let unlimited = self.bytes.get(self.pos) == Some(&b'*');
        let repeat = if unlimited {
            self.pos += 1;
            1
        } else {
            self.number()?.unwrap_or(1)
        };
        if self.bytes.get(self.pos) != Some(&b'(') {
            self.pos = saved;
            return Ok(None);
        }
        if repeat == 0 || repeat > 10000 {
            return Err("output descriptor exceeds supported limits");
        }
        self.pos += 1;
        let start = self.pos;
        let mut nesting = 1;
        let mut quote = 0;
        while let Some(&ch) = self.bytes.get(self.pos) {
            if quote != 0 {
                if ch == quote {
                    if self.bytes.get(self.pos + 1) == Some(&quote) {
                        self.pos += 1;
                    } else {
                        quote = 0;
                    }
                }
            } else if ch == b'\'' || ch == b'"' {
                quote = ch;
            } else if ch == b'(' {
                nesting += 1;
            } else if ch == b')' {
                nesting -= 1;
                if nesting == 0 {
                    break;
                }
            }
            self.pos += 1;
        }
        if nesting != 0 {
            return Err("unterminated output format group");
        }
        let body = &self.bytes[start..self.pos];
        self.pos += 1;
        self.spaces();
        if self
            .bytes
            .get(self.pos)
            .is_some_and(|c| *c != b',' && *c != b'/')
        {
            return Err("expected comma between output descriptors");
        }
        Ok(Some((body, repeat, unlimited)))
    }
}
fn validate_groups(mut parser: Parser<'_>, depth: usize) -> Result<bool, &'static str> {
    if depth >= 32 {
        return Err("output format nesting exceeds supported limits");
    }
    let mut data = false;
    loop {
        parser.separator();
        if parser.pos == parser.bytes.len() {
            return Ok(data);
        }
        if let Some((body, _, unlimited)) = parser.group()? {
            let inner = validate_groups(
                Parser {
                    bytes: body,
                    pos: 0,
                },
                depth + 1,
            )?;
            if unlimited {
                if depth != 0 || parser.pos != parser.bytes.len() {
                    return Err("unlimited output format group must be last and outermost");
                }
                if !inner {
                    return Err("unlimited output format group requires a data descriptor");
                }
            }
            data |= inner;
        } else if let Some(d) = parser.next()? {
            data |= matches!(d.code, b'a' | b'i' | b'f' | b'g' | b'l');
        }
    }
}
impl<'a> GroupParser<'a> {
    pub fn new(bytes: &'a [u8], count: usize) -> Result<Self, &'static str> {
        let parser = Parser::new(bytes)?;
        validate_groups(parser, 0)?;
        let empty = Frame {
            parser: Parser { bytes: &[], pos: 0 },
            remaining: 0,
            unlimited: false,
        };
        let mut frames = [empty; 32];
        frames[0] = Frame {
            parser,
            remaining: 1,
            unlimited: false,
        };
        Ok(Self {
            frames,
            depth: 1,
            count,
            consumed: 0,
            operations: 0,
        })
    }
    pub fn next(&mut self) -> Result<Option<Descriptor<'a>>, &'static str> {
        while self.depth > 0 {
            self.operations += 1;
            if self.operations > self.count.saturating_mul(256).saturating_add(100000) {
                return Err("output format expansion exceeds supported limits");
            }
            let index = self.depth - 1;
            self.frames[index].parser.separator();
            if self.frames[index].parser.pos == self.frames[index].parser.bytes.len() {
                let frame = &mut self.frames[index];
                if (frame.unlimited && self.consumed < self.count)
                    || (!frame.unlimited && frame.remaining > 1)
                {
                    if !frame.unlimited {
                        frame.remaining -= 1;
                    }
                    frame.parser.pos = 0;
                } else {
                    self.depth -= 1;
                }
                continue;
            }
            if let Some((body, repeat, unlimited)) = self.frames[index].parser.group()? {
                if unlimited && self.consumed >= self.count {
                    continue;
                }
                self.frames[self.depth] = Frame {
                    parser: Parser {
                        bytes: body,
                        pos: 0,
                    },
                    remaining: repeat,
                    unlimited,
                };
                self.depth += 1;
                continue;
            }
            if let Some(mut d) = self.frames[index].parser.next()? {
                // Count expanded descriptors, not only group traversal. Nested
                // literal/spacing repeats must not create unbounded field lists.
                self.operations = self.operations.saturating_add(d.repeat);
                if self.operations > self.count.saturating_mul(256).saturating_add(100000) {
                    return Err("output format expansion exceeds supported limits");
                }
                if d.code==b':' {
                    if d.repeat!=1 {return Err("colon edit descriptor cannot have a repeat count");}
                    if self.consumed>=self.count {self.depth=0;return Ok(None);}
                    continue;
                }
                if matches!(d.code, b'a' | b'i' | b'f' | b'g' | b'l') {
                    if self.frames[..self.depth].iter().any(|f| f.unlimited) {
                        if self.consumed >= self.count {
                            self.depth = 0;
                            return Ok(None);
                        }
                        d.repeat = d.repeat.min(self.count - self.consumed);
                    }
                    self.consumed = self
                        .consumed
                        .checked_add(d.repeat)
                        .ok_or("output descriptor exceeds supported limits")?;
                }
                return Ok(Some(d));
            }
        }
        Ok(None)
    }
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Statement-scoped list-directed input, shared by the interpreter and native runtime.
//! Only tokens are buffered: record length is independent of destination size.
use core::{ptr, slice, str};
extern "C" {
    fn realloc(p: *mut core::ffi::c_void, n: usize) -> *mut core::ffi::c_void;
    fn free(p: *mut core::ffi::c_void);
}
struct Bytes {
    p: *mut u8,
    len: usize,
    cap: usize,
}
impl Bytes {
    fn new() -> Self {
        Self {
            p: ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }
    fn push(&mut self, c: u8) -> Result<(), i64> {
        if c == 0 || c > 127 {
            return Err(2);
        }
        if self.len == self.cap {
            let cap = self.cap.checked_mul(2).unwrap_or(0).max(64);
            if cap <= self.cap {
                return Err(1);
            }
            let p: *mut u8 = unsafe { realloc(self.p.cast(), cap) }.cast();
            if p.is_null() {
                return Err(1);
            }
            self.p = p;
            self.cap = cap;
        }
        unsafe {
            *self.p.add(self.len) = c;
        }
        self.len += 1;
        Ok(())
    }
    fn bytes(&self) -> &[u8] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(self.p, self.len) }
        }
    }
}
impl Drop for Bytes {
    fn drop(&mut self) {
        unsafe {
            free(self.p.cast());
        }
    }
}
pub(crate) struct Session {
    token: Bytes,
    look: Option<u8>,
    eof: bool,
    after: bool,
    ended: bool,
    remaining: u64,
    repeat_kind: i64,
    null: bool,
    quoted: bool,
    at_record_end: bool,
}
impl Session {
    pub(crate) fn new() -> Self {
        Self {
            token: Bytes::new(),
            look: None,
            eof: false,
            after: false,
            ended: false,
            remaining: 0,
            repeat_kind: 0,
            null: false,
            quoted: false,
            at_record_end: false,
        }
    }
    fn peek(
        &mut self,
        byte: &mut impl FnMut() -> Result<Option<u8>, ()>,
    ) -> Result<Option<u8>, i64> {
        if self.look.is_none() && !self.eof {
            self.look = byte().map_err(|_| 1)?;
            self.eof = self.look.is_none();
        }
        Ok(self.look)
    }
    fn take(
        &mut self,
        byte: &mut impl FnMut() -> Result<Option<u8>, ()>,
    ) -> Result<Option<u8>, i64> {
        let c = self.peek(byte)?;
        self.look = None;
        if let Some(c) = c {
            self.at_record_end = c == b'\n';
        }
        Ok(c)
    }
    fn spaces(&mut self, byte: &mut impl FnMut() -> Result<Option<u8>, ()>) -> Result<(), i64> {
        while self.peek(byte)?.is_some_and(|c| c.is_ascii_whitespace()) {
            self.take(byte)?;
        }
        Ok(())
    }
    fn scan(
        &mut self,
        kind: i64,
        byte: &mut impl FnMut() -> Result<Option<u8>, ()>,
    ) -> Result<(), i64> {
        if self.remaining > 0 {
            self.remaining -= 1;
            if !self.null && kind != self.repeat_kind {
                return Err(2);
            }
            return Ok(());
        }
        self.repeat_kind = kind;
        self.token.len = 0;
        self.null = false;
        self.quoted = false;
        self.spaces(byte)?;
        if self.after && self.peek(byte)? == Some(b',') {
            self.take(byte)?;
            self.spaces(byte)?;
        }
        self.after = true;
        match self.peek(byte)? {
            None => return Err(-1),
            Some(b'/') => {
                self.take(byte)?;
                self.ended = true;
                self.null = true;
                return Ok(());
            }
            Some(b',') => {
                self.null = true;
                return Ok(());
            }
            _ => {}
        }
        // A repetition prefix is lexical and must be preserved across differently typed items.
        while self.peek(byte)?.is_some_and(|c| c.is_ascii_digit()) {
            let c = self.take(byte)?.unwrap();
            self.token.push(c)?;
        }
        if self.token.len > 0 && self.peek(byte)? == Some(b'*') {
            let n = str::from_utf8(self.token.bytes())
                .map_err(|_| 2)?
                .parse::<u64>()
                .map_err(|_| 2)?;
            if n == 0 {
                return Err(2);
            }
            self.remaining = n - 1;
            self.token.len = 0;
            self.take(byte)?;
            if self.peek(byte)?.is_none_or(separator) {
                self.null = true;
                return Ok(());
            }
        }
        if self.token.len == 0 && matches!(self.peek(byte)?, Some(b'\'' | b'"')) {
            self.quoted = true;
            let quote = self.take(byte)?.unwrap();
            loop {
                let c = self.take(byte)?.ok_or(2)?;
                if c == quote {
                    if self.peek(byte)? == Some(quote) {
                        self.take(byte)?;
                        self.token.push(quote)?;
                    } else {
                        break;
                    }
                } else if c != b'\n' && c != b'\r' {
                    self.token.push(c)?;
                }
            }
            if self.peek(byte)?.is_some_and(|c| !separator(c)) {
                return Err(2);
            }
        } else {
            let mut depth = 0;
            while let Some(c) = self.peek(byte)? {
                if depth == 0 && separator(c) {
                    break;
                }
                self.take(byte)?;
                if kind == 5 {
                    if c == b'(' {
                        depth += 1;
                    }
                    if c == b')' {
                        depth -= 1;
                        if depth < 0 {
                            return Err(2);
                        }
                    }
                }
                self.token.push(c)?;
            }
            if depth != 0 || self.token.len == 0 {
                return Err(2);
            }
        }
        Ok(())
    }
    /// 0 = transferred, -2 = null/slash (destination unchanged), -1 = EOF, positive = error.
    pub(crate) fn next(
        &mut self,
        kind: i64,
        width: usize,
        out: &mut [u8],
        mut byte: impl FnMut() -> Result<Option<u8>, ()>,
    ) -> i64 {
        if self.ended {
            return -2;
        }
        match self
            .scan(kind, &mut byte)
            .and_then(|()| self.convert(kind, width, out))
        {
            Ok(()) => {
                if self.null {
                    -2
                } else {
                    0
                }
            }
            Err(code) => code,
        }
    }
    fn convert(&mut self, kind: i64, width: usize, out: &mut [u8]) -> Result<(), i64> {
        if self.null {
            return Ok(());
        }
        let text = str::from_utf8(self.token.bytes()).map_err(|_| 2)?;
        if kind != 4 && self.quoted {
            return Err(2);
        }
        match kind {
            1 => {
                let n = text.parse::<i64>().map_err(|_| 2)?;
                if width < 8 {
                    let bits = width * 8;
                    if n < -(1i64 << (bits - 1)) || n > (1i64 << (bits - 1)) - 1 {
                        return Err(2);
                    }
                }
                out[..8].copy_from_slice(&n.to_ne_bytes());
            }
            2 => {
                let n = real_value(self.token.bytes())?;
                let n = checked_real(n, width, self.token.bytes())?;
                out[..8].copy_from_slice(&n.to_ne_bytes());
            }
            5 => {
                let text = text
                    .strip_prefix('(')
                    .and_then(|s| s.strip_suffix(')'))
                    .ok_or(2)?;
                let (re, im) = text.split_once(',').ok_or(2)?;
                let re = real_value(re.trim().as_bytes())?;
                let im = real_value(im.trim().as_bytes())?;
                let re =
                    checked_real(re, width, text.split_once(',').unwrap().0.trim().as_bytes())?;
                let im =
                    checked_real(im, width, text.split_once(',').unwrap().1.trim().as_bytes())?;
                out[..8].copy_from_slice(&re.to_ne_bytes());
                out[8..16].copy_from_slice(&im.to_ne_bytes());
            }
            3 => {
                let text = text.strip_prefix('.').unwrap_or(text);
                let n = match text.as_bytes().first().map(u8::to_ascii_lowercase) {
                    Some(b't') => 1i64,
                    Some(b'f') => 0,
                    _ => return Err(2),
                };
                out[..8].copy_from_slice(&n.to_ne_bytes());
            }
            4 => {
                out.fill(b' ');
                let n = width.min(self.token.len);
                out[..n].copy_from_slice(&self.token.bytes()[..n]);
            }
            _ => return Err(2),
        }
        Ok(())
    }
    pub(crate) fn finish(&mut self, mut byte: impl FnMut() -> Result<Option<u8>, ()>) -> i64 {
        // A token may have looked ahead at EOR; consume it, but never the next record.
        let mut consumed = self.after;
        while self.look.is_some() || !self.at_record_end {
            match self.take(&mut byte) {
                Ok(Some(b'\n')) => break,
                Ok(None) => return if consumed { 0 } else { -1 },
                Ok(Some(_)) => {
                    consumed = true;
                }
                Err(code) => return code,
            }
        }
        0
    }
}
fn checked_real(value: f64, width: usize, source: &[u8]) -> Result<f64, i64> {
    let value = if width == 4 {
        (value as f32) as f64
    } else {
        value
    };
    let source = source
        .strip_prefix(b"+")
        .or_else(|| source.strip_prefix(b"-"))
        .unwrap_or(source);
    if value.is_infinite()
        && !source.eq_ignore_ascii_case(b"inf")
        && !source.eq_ignore_ascii_case(b"infinity")
    {
        return Err(2);
    }
    Ok(value)
}
fn real_value(source: &[u8]) -> Result<f64, i64> {
    let mut normalized = Bytes::new();
    let mut exponent = false;
    for (i, &c) in source.iter().enumerate() {
        let c = if matches!(c, b'd' | b'D') { b'e' } else { c };
        if matches!(c, b'e' | b'E') {
            exponent = true;
        }
        if i > 0 && matches!(c, b'+' | b'-') && !exponent {
            normalized.push(b'e')?;
            exponent = true;
        }
        normalized.push(c)?;
    }
    str::from_utf8(normalized.bytes())
        .map_err(|_| 2)?
        .parse::<f64>()
        .map_err(|_| 2)
}
fn separator(c: u8) -> bool {
    c.is_ascii_whitespace() || matches!(c, b',' | b'/')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_repeats_nulls_and_slash() {
        let mut input = b"1,2*3, ,\n4 / discard\n9\n".iter().copied();
        let mut s = Session::new();
        let mut out = [0; 8];
        for value in [1, 3, 3] {
            assert_eq!(s.next(1, 4, &mut out, || Ok(input.next())), 0);
            assert_eq!(i64::from_ne_bytes(out), value);
        }
        assert_eq!(s.next(1, 4, &mut out, || Ok(input.next())), -2);
        assert_eq!(s.next(1, 4, &mut out, || Ok(input.next())), 0);
        assert_eq!(i64::from_ne_bytes(out), 4);
        assert_eq!(s.next(1, 4, &mut out, || Ok(input.next())), -2);
        s.finish(|| Ok(input.next()));
        assert_eq!(input.next(), Some(b'9'));
    }
}

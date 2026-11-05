// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Runtime defined-I/O cursor shared by native code and the interpreter.
//! The cursor owns its format bytes; parser slices never refer to caller storage.
use crate::format_spec::{self as f, Descriptor, GroupParser};
use core::{ptr, slice};
extern "C" {
    fn malloc(n: usize) -> *mut core::ffi::c_void;
    fn free(p: *mut core::ffi::c_void);
}
pub const CAP: usize = 4096;
struct Text {
    bytes: [u8; CAP],
    len: usize,
}
impl Text {
    fn new() -> Self {
        Self {
            bytes: [b' '; CAP],
            len: 0,
        }
    }
    fn add(&mut self, s: &[u8]) -> Result<(), &'static str> {
        if self.len + s.len() > CAP {
            return Err("defined I/O format expansion exceeds 4096 bytes");
        }
        self.bytes[self.len..self.len + s.len()].copy_from_slice(s);
        self.len += s.len();
        Ok(())
    }
    fn get(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
struct Cursor {
    source: *mut u8,
    parser: GroupParser<'static>,
    repeated: Option<Descriptor<'static>>,
    remaining: usize,
    before: Text,
    data: Text,
    iotype: Text,
    scale: Text,
    sign: Text,
    blank: Text,
    values: [i64; 128],
    count: usize,
}
impl Drop for Cursor {
    fn drop(&mut self) {
        unsafe { free(self.source.cast()) }
    }
}
impl Cursor {
    unsafe fn new(bytes: &[u8]) -> Result<Self, &'static str> {
        let source = malloc(bytes.len().max(1)).cast::<u8>();
        if source.is_null() {
            return Err("cannot allocate defined I/O format");
        }
        ptr::copy_nonoverlapping(bytes.as_ptr(), source, bytes.len());
        let mut parser =
            match GroupParser::new(slice::from_raw_parts(source, bytes.len()), usize::MAX) {
                Ok(p) => p,
                Err(e) => {
                    free(source.cast());
                    return Err(e);
                }
            };
        parser.preserve_boundaries();
        Ok(Self {
            source,
            parser,
            repeated: None,
            remaining: 0,
            before: Text::new(),
            data: Text::new(),
            iotype: Text::new(),
            scale: Text::new(),
            sign: Text::new(),
            blank: Text::new(),
            values: [0; 128],
            count: 0,
        })
    }
    fn step(&mut self, finish: bool) -> Result<i64, &'static str> {
        self.before = Text::new();
        self.data = Text::new();
        self.iotype = Text::new();
        self.count = 0;
        self.before.add(b"(")?;
        // Bound work per transfer, rather than the total array size.
        self.parser.reset_operation_count();
        for _ in 0..10000 {
            let d = if self.remaining > 0 {
                self.remaining -= 1;
                self.repeated.unwrap()
            } else {
                let d = self
                    .parser
                    .next()?
                    .ok_or("defined I/O format has no next data descriptor")?;
                if f::is_data(d.code) {
                    self.remaining = d.repeat - 1;
                    self.repeated = Some(d);
                }
                d
            };
            if finish && (f::is_data(d.code) || matches!(d.code, b':' | f::REVERSION)) {
                self.before.add(b")")?;
                return Ok(0);
            }
            if matches!(d.code, f::T | f::TL) {
                return Err("T/TL positioning across defined I/O transfers is not yet supported");
            }
            if f::is_data(d.code) {
                self.before.add(b")")?;
                self.data.add(b"(")?;
                self.data.add(self.scale.get())?;
                self.data.add(self.sign.get())?;
                self.data.add(self.blank.get())?;
                self.data.add(&self.before.get()[1..self.before.len - 1])?;
                self.data.add(d.raw)?;
                self.data.add(b")")?;
                if d.code == f::DT {
                    self.metadata(d.raw)?;
                }
                return Ok(d.code as i64);
            }
            if d.code == b':' {
                continue;
            }
            if d.code == f::SCALE {
                self.scale = Text::new();
                self.scale.add(d.raw)?;
                self.scale.add(b",")?;
            }
            if matches!(d.code, f::SIGN_PLUS | f::SIGN_DEFAULT) {
                self.sign = Text::new();
                self.sign.add(d.raw)?;
                self.sign.add(b",")?;
            }
            if matches!(d.code, f::BLANK_NULL | f::BLANK_ZERO) {
                self.blank = Text::new();
                self.blank.add(d.raw)?;
                self.blank.add(b",")?;
            }
            self.before.add(d.raw)?;
            self.before.add(b",")?;
        }
        Err("defined I/O format control expansion exceeds supported limits")
    }
    fn metadata(&mut self, raw: &[u8]) -> Result<(), &'static str> {
        let text = core::str::from_utf8(raw).map_err(|_| "invalid DT descriptor")?;
        let text = text.trim_start_matches(|c: char| c.is_ascii_digit() || c.is_ascii_whitespace());
        let mut rest = text[2..].trim();
        self.iotype.add(b"DT")?;
        if rest.starts_with(['\'', '"']) {
            let quote = rest.as_bytes()[0];
            let bytes = rest.as_bytes();
            let mut p = 1;
            loop {
                let c = *bytes.get(p).ok_or("unterminated DT string")?;
                p += 1;
                if c == quote {
                    if bytes.get(p) == Some(&quote) {
                        p += 1;
                    } else {
                        break;
                    }
                }
                self.iotype.add(&[c])?;
            }
            rest = rest[p..].trim();
        }
        if !rest.is_empty() {
            let list = rest
                .strip_prefix('(')
                .and_then(|s| s.strip_suffix(')'))
                .ok_or("invalid DT v-list")?;
            if !list.trim().is_empty() {
                for value in list.split(',') {
                    if self.count == 128 {
                        return Err("runtime DT v-list exceeds 128 integers");
                    }
                    self.values[self.count] = value
                        .trim()
                        .parse::<i32>()
                        .map_err(|_| "DT v-list requires default INTEGER literals")?
                        as i64;
                    self.count += 1;
                }
            }
        }
        Ok(())
    }
}
fn put(dest: &mut [u8], text: &[u8]) -> Result<(), &'static str> {
    if text.len() > dest.len() {
        return Err("defined I/O format buffer is too short");
    }
    dest.fill(b' ');
    dest[..text.len()].copy_from_slice(text);
    Ok(())
}
/// Internal ABI: 0=open, 1=next, 2=prefix, 3=field format, 4=iotype,
/// 5=v-list size, 6=v-list value (one based), 7=finish, 8=close.
/// Only compiler-generated calls may supply handles.
pub unsafe fn operate(handle: &mut i64, op: i64, value: &mut i64, text: &mut [u8]) -> i64 {
    let result = (|| -> Result<(), &'static str> {
        if op == 0 {
            *handle = 0;
            let cursor = Cursor::new(text)?;
            let p = malloc(core::mem::size_of::<Cursor>()).cast::<Cursor>();
            if p.is_null() {
                return Err("cannot allocate defined I/O cursor");
            }
            ptr::write(p, cursor);
            *handle = p as i64;
            return Ok(());
        }
        if op == 8 {
            if *handle != 0 {
                let p = *handle as *mut Cursor;
                ptr::drop_in_place(p);
                free(p.cast());
                *handle = 0;
            }
            return Ok(());
        }
        if *handle == 0 {
            return Err("invalid defined I/O format cursor");
        }
        let c = &mut *(*handle as *mut Cursor);
        match op {
            1 => *value = c.step(false)?,
            2 => put(text, c.before.get())?,
            3 => put(text, c.data.get())?,
            4 => put(text, c.iotype.get())?,
            5 => *value = c.count as i64,
            6 => {
                if *value < 1 || *value > c.count as i64 {
                    return Err("invalid DT v-list index");
                }
                *value = c.values[*value as usize - 1];
            }
            7 => {
                c.step(true)?;
                put(text, c.before.get())?;
            }
            _ => return Err("invalid defined I/O cursor operation"),
        }
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => {
            text.fill(b' ');
            let n = text.len().min(e.len());
            text[..n].copy_from_slice(&e.as_bytes()[..n]);
            1
        }
    }
}

/// Read one field (or just the controls preceding DT) from the parent's session.
pub unsafe fn input(
    handle: i64,
    tag: i64,
    dest_len: usize,
    text: &mut [u8],
    mut fields: impl FnMut(&mut [u8], bool) -> i64,
) -> i64 {
    let result = (|| -> Result<(), i64> {
        if handle == 0 {
            return Err(1);
        }
        let c = &*(handle as *const Cursor);
        let source = if tag == 0 {
            c.before.get()
        } else {
            c.data.get()
        };
        let mut parser = GroupParser::new(source, if tag == 0 { 0 } else { 1 }).map_err(|_| 1)?;
        let mut buffer = [b' '; CAP];
        let mut blank_zero = false;
        let mut scale = 0;
        while let Some(d) = parser.next().map_err(|_| 1)? {
            if matches!(d.code, b'x' | f::TR) {
                let mut left = d.width.max(1).checked_mul(d.repeat).ok_or(1)?;
                while left > 0 {
                    let n = left.min(CAP);
                    let code = fields(&mut buffer[..n], false);
                    if code != 0 {
                        return Err(code);
                    }
                    left -= n;
                }
            } else if matches!(d.code, b'/' | f::REVERSION) {
                for _ in 0..d.repeat {
                    let code = fields(&mut [], true);
                    if code != 0 {
                        return Err(code);
                    }
                }
            } else if d.code == f::BLANK_ZERO {
                blank_zero = true;
            } else if d.code == f::BLANK_NULL {
                blank_zero = false;
            } else if d.code == f::SCALE {
                scale = d.scale;
            } else if matches!(d.code, f::SIGN_PLUS | f::SIGN_DEFAULT | b':') {
            } else if f::is_data(d.code) {
                if !f::matches_type(d.code, tag as u64) {
                    return Err(1);
                }
                let width = if d.code == b'a' && d.width == 0 {
                    dest_len
                } else {
                    d.width
                };
                if (width == 0 && tag != 4) || width > CAP {
                    return Err(1);
                }
                let code = fields(&mut buffer[..width], false);
                if code != 0 {
                    return Err(code);
                }
                text.fill(b' ');
                if tag == 4 {
                    let field = if width > dest_len {
                        &buffer[width - dest_len..width]
                    } else {
                        &buffer[..width]
                    };
                    if field.len() > text.len() {
                        return Err(1);
                    }
                    text[..field.len()].copy_from_slice(field);
                    return Ok(());
                }
                let mut normalized = Text::new();
                for &b in &buffer[..width] {
                    if b == b' ' {
                        if blank_zero {
                            normalized.add(b"0").map_err(|_| 1)?;
                        }
                    } else {
                        normalized.add(&[b]).map_err(|_| 1)?;
                    }
                }
                if normalized.len == 0 {
                    normalized
                        .add(if tag == 3 { b"F" } else { b"0" })
                        .map_err(|_| 1)?;
                }
                if matches!(d.code, b'b' | b'o' | b'z') {
                    let radix = match d.code {
                        b'b' => 2,
                        b'o' => 8,
                        _ => 16,
                    };
                    let v = i64::from_str_radix(
                        core::str::from_utf8(normalized.get()).map_err(|_| 1)?,
                        radix,
                    )
                    .map_err(|_| 1)?;
                    normalized = Text::new();
                    use core::fmt::Write;
                    write!(&mut normalized, "{v}").map_err(|_| 1)?;
                } else if tag == 2 {
                    let exp = normalized
                        .get()
                        .iter()
                        .enumerate()
                        .find(|(_, b)| matches!(b, b'e' | b'E' | b'd' | b'D'))
                        .map(|(i, _)| (i, i + 1))
                        .or_else(|| {
                            normalized
                                .get()
                                .iter()
                                .enumerate()
                                .skip(1)
                                .find(|(_, b)| matches!(b, b'+' | b'-'))
                                .map(|(i, _)| (i, i))
                        });
                    let old = normalized;
                    normalized = Text::new();
                    let (mantissa, exponent) = if let Some((end, start)) = exp {
                        (
                            &old.get()[..end],
                            core::str::from_utf8(&old.get()[start..])
                                .map_err(|_| 1)?
                                .parse::<i32>()
                                .map_err(|_| 1)? as i64,
                        )
                    } else {
                        (old.get(), -(scale as i64))
                    };
                    let decimals = if mantissa.contains(&b'.') {
                        0
                    } else {
                        i64::try_from(d.precision).map_err(|_| 1)?
                    };
                    normalized.add(mantissa).map_err(|_| 1)?;
                    use core::fmt::Write;
                    write!(
                        &mut normalized,
                        "e{}",
                        exponent.checked_sub(decimals).ok_or(1)?
                    )
                    .map_err(|_| 1)?;
                }
                put(text, normalized.get()).map_err(|_| 1)?;
                return Ok(());
            } else {
                return Err(1);
            }
        }
        if tag != 0 {
            return Err(1);
        }
        Ok(())
    })();
    result.err().unwrap_or(0)
}
impl core::fmt::Write for Text {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.add(s.as_bytes()).map_err(|_| core::fmt::Error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_owns_format_and_has_no_total_iteration_limit() {
        unsafe {
            let mut h = 0;
            let mut v = 0;
            let mut text = [b' '; CAP];
            text[..7].copy_from_slice(b"(*(dt))");
            assert_eq!(operate(&mut h, 0, &mut v, &mut text), 0);
            text.fill(b'x');
            for _ in 0..100010 {
                assert_eq!(operate(&mut h, 1, &mut v, &mut text), 0);
                assert_eq!(v, f::DT as i64);
            }
            assert_eq!(operate(&mut h, 7, &mut v, &mut text), 0);
            assert_eq!(&text[..2], b"()");
            assert_eq!(operate(&mut h, 8, &mut v, &mut text), 0);
            assert_eq!(h, 0);
        }
    }
    #[test]
    fn signed_metadata_and_bounds() {
        unsafe {
            let mut h = 0;
            let mut v = 0;
            let mut text = [b' '; CAP];
            let format = b"(dt'a''b'(-3,+2,0))";
            text[..format.len()].copy_from_slice(format);
            assert_eq!(operate(&mut h, 0, &mut v, &mut text), 0);
            assert_eq!(operate(&mut h, 1, &mut v, &mut text), 0);
            assert_eq!(operate(&mut h, 4, &mut v, &mut text), 0);
            assert_eq!(&text[..5], b"DTa'b");
            assert_eq!(operate(&mut h, 5, &mut v, &mut text), 0);
            assert_eq!(v, 3);
            for (i, expected) in [-3, 2, 0].iter().enumerate() {
                v = i as i64 + 1;
                assert_eq!(operate(&mut h, 6, &mut v, &mut text), 0);
                assert_eq!(v, *expected);
            }
            v = 4;
            assert_eq!(operate(&mut h, 6, &mut v, &mut text), 1);
            assert_eq!(operate(&mut h, 8, &mut v, &mut text), 0);
        }
    }
}

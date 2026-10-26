// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Streaming NAMELIST lexical reader shared by native execution and interpretation.
//! Tokens have an explicit 4096-byte limit; overlong input is an error, never truncated.
pub(crate) struct Session {
    group: [u8; 128],
    group_len: usize,
    field: [u8; 128],
    field_len: usize,
    token: [u8; 4096],
    length: usize,
    look: Option<u8>,
    found: bool,
    ended: bool,
    after: bool,
    index: i64,
    repeat: u64,
    null: bool,
    subscripts: [i64; 15],
    rank: usize,
    new_assignment: bool,
    apply: bool,
    stride: i64,
}
impl Session {
    pub(crate) fn new(group: &[u8]) -> Self {
        let mut s = Self {
            group: [0; 128],
            group_len: group.len(),
            field: [0; 128],
            field_len: 0,
            token: [0; 4096],
            length: 0,
            look: None,
            found: false,
            ended: false,
            after: false,
            index: 0,
            repeat: 0,
            null: false,
            subscripts: [0; 15],
            rank: 0,
            new_assignment: false,
            apply: false,
            stride: 1,
        };
        if group.len() <= 128 {
            for (a, b) in s.group.iter_mut().zip(group) {
                *a = b.to_ascii_lowercase();
            }
        }
        s
    }
    fn peek(
        &mut self,
        read: &mut impl FnMut() -> Result<Option<u8>, ()>,
    ) -> Result<Option<u8>, i64> {
        if self.look.is_none() {
            self.look = read().map_err(|_| 1)?;
        }
        if self.look.is_some_and(|b| b == 0 || b > 127) {
            return Err(1);
        }
        Ok(self.look)
    }
    fn get(
        &mut self,
        read: &mut impl FnMut() -> Result<Option<u8>, ()>,
    ) -> Result<Option<u8>, i64> {
        let c = self.peek(read)?;
        self.look = None;
        Ok(c)
    }
    fn spaces(&mut self, read: &mut impl FnMut() -> Result<Option<u8>, ()>) -> Result<(), i64> {
        loop {
            match self.peek(read)? {
                Some(b'!') => {
                    while let Some(c) = self.get(read)? {
                        if c == b'\n' {
                            break;
                        }
                    }
                }
                Some(c) if c.is_ascii_whitespace() => {
                    self.get(read)?;
                }
                _ => return Ok(()),
            }
        }
    }
    fn push(&mut self, b: u8) -> Result<(), i64> {
        if self.length == self.token.len() {
            return Err(6);
        }
        self.token[self.length] = b;
        self.length += 1;
        Ok(())
    }
    fn quoted(
        &mut self,
        quote: u8,
        read: &mut impl FnMut() -> Result<Option<u8>, ()>,
    ) -> Result<(), i64> {
        self.push(quote)?;
        loop {
            let c = self.get(read)?.ok_or(-1)?;
            self.push(c)?;
            if c == quote {
                if self.peek(read)? == Some(quote) {
                    self.get(read)?;
                    self.push(quote)?;
                } else {
                    return Ok(());
                }
            }
        }
    }
    fn finish_record(
        &mut self,
        read: &mut impl FnMut() -> Result<Option<u8>, ()>,
    ) -> Result<(), i64> {
        while let Some(c) = self.get(read)? {
            if c == b'\n' {
                break;
            }
        }
        Ok(())
    }
    fn search(&mut self, read: &mut impl FnMut() -> Result<Option<u8>, ()>) -> Result<(), i64> {
        if self.group_len > 128 {
            return Err(1);
        }
        loop {
            self.spaces(read)?;
            let c = self.get(read)?.ok_or(-1)?;
            if c == b'\'' || c == b'"' {
                self.length = 0;
                self.quoted(c, read)?;
                continue;
            }
            if c != b'&' {
                continue;
            }
            self.length = 0;
            while let Some(c) = self.peek(read)? {
                if c.is_ascii_alphanumeric() || c == b'_' {
                    self.get(read)?;
                    self.push(c.to_ascii_lowercase())?;
                } else {
                    break;
                }
            }
            if self.token[..self.length] == self.group[..self.group_len] {
                self.found = true;
                return Ok(());
            }
            // Skip a different group, honoring strings and comments.
            loop {
                self.spaces(read)?;
                let c = self.get(read)?.ok_or(-1)?;
                if c == b'/' {
                    break;
                }
                if c == b'\'' || c == b'"' {
                    self.length = 0;
                    self.quoted(c, read)?;
                }
            }
        }
    }
    fn event(&mut self, read: &mut impl FnMut() -> Result<Option<u8>, ()>) -> Result<i64, i64> {
        if self.ended {
            return Ok(-2);
        }
        if !self.found {
            self.search(read)?;
        }
        if self.repeat > 0 {
            self.repeat -= 1;
            self.index = self.index.checked_add(1).ok_or(4)?;
            return Ok(if self.null { 2 } else { 0 });
        }
        loop {
            self.spaces(read)?;
            if self.after && self.peek(read)? == Some(b',') {
                self.get(read)?;
                self.spaces(read)?;
            }
            self.after = false;
            let c = self.get(read)?.ok_or(-1)?;
            if c == b'/' {
                self.ended = true;
                self.finish_record(read)?;
                return Ok(-2);
            }
            self.length = 0;
            self.null = false;
            if c == b',' {
                // Leave this comma as the separator following the null event.
                self.look = Some(b',');
                self.null = true;
            } else if c == b'\'' || c == b'"' {
                self.quoted(c, read)?;
            } else if c == b'(' {
                self.push(c)?;
                loop {
                    let c = self.get(read)?.ok_or(-1)?;
                    self.push(c)?;
                    if c == b')' {
                        break;
                    }
                }
            } else {
                self.push(c)?;
                while let Some(c) = self.peek(read)? {
                    if c.is_ascii_whitespace()
                        || matches!(c, b',' | b'/' | b'=' | b'!' | b'(' | b'\'' | b'"')
                    {
                        break;
                    }
                    self.get(read)?;
                    self.push(c)?;
                }
                self.spaces(read)?;
                let designator = self.peek(read)? == Some(b'=')
                    || self.peek(read)? == Some(b'(') && self.token[0].is_ascii_alphabetic();
                if designator {
                    if self.length == 0
                        || self.length > 128
                        || !self.token[0].is_ascii_alphabetic()
                        || self.token[..self.length]
                            .iter()
                            .any(|c| !c.is_ascii_alphanumeric() && *c != b'_')
                    {
                        return Err(3);
                    }
                    self.field_len = self.length;
                    for j in 0..self.length {
                        self.field[j] = self.token[j].to_ascii_lowercase();
                    }
                    self.rank = 0;
                    if self.peek(read)? == Some(b'(') {
                        self.get(read)?;
                        loop {
                            self.spaces(read)?;
                            self.length = 0;
                            if let Some(c @ (b'+' | b'-')) = self.peek(read)? {
                                self.get(read)?;
                                self.push(c)?;
                            }
                            while let Some(c) = self.peek(read)? {
                                if !c.is_ascii_digit() {
                                    break;
                                }
                                self.get(read)?;
                                self.push(c)?;
                            }
                            if self.rank == 15 {
                                return Err(7);
                            }
                            self.subscripts[self.rank] =
                                core::str::from_utf8(&self.token[..self.length])
                                    .map_err(|_| 7)?
                                    .parse::<i64>()
                                    .map_err(|_| 7)?;
                            self.rank += 1;
                            self.spaces(read)?;
                            match self.get(read)? {
                                Some(b',') => continue,
                                Some(b')') => break,
                                _ => return Err(7),
                            }
                        }
                        self.spaces(read)?;
                    }
                    if self.get(read)? != Some(b'=') {
                        return Err(7);
                    }
                    self.index = 0;
                    self.new_assignment = true;
                    continue;
                }
                if let Some(star) = self.token[..self.length].iter().position(|c| *c == b'*') {
                    let prefix = core::str::from_utf8(&self.token[..star]).map_err(|_| 1)?;
                    let repeat = prefix.parse::<u64>().map_err(|_| 1)?;
                    if repeat == 0 {
                        return Err(1);
                    }
                    self.repeat = repeat - 1;
                    self.token.copy_within(star + 1..self.length, 0);
                    self.length -= star + 1;
                    if self.length == 0 {
                        match self.peek(read)? {
                            Some(q @ (b'\'' | b'"')) => {
                                self.get(read)?;
                                self.quoted(q, read)?;
                            }
                            Some(b'(') => {
                                self.get(read)?;
                                self.push(b'(')?;
                                loop {
                                    let c = self.get(read)?.ok_or(-1)?;
                                    self.push(c)?;
                                    if c == b')' {
                                        break;
                                    }
                                }
                            }
                            _ => self.null = true,
                        }
                    }
                }
            }
            if self.field_len == 0 {
                return Err(3);
            }
            self.after = true;
            self.index = self.index.checked_add(1).ok_or(4)?;
            return Ok(if self.null { 2 } else { 0 });
        }
    }
    /// Resolve an optional element designator with the compiler's original
    /// lower bounds. Subsequent values advance in Fortran array element order.
    pub(crate) fn layout(
        &mut self,
        index: &mut i64,
        code: i64,
        axis: i64,
        rank: i64,
        lower: i64,
        upper: i64,
    ) -> i64 {
        if code != 0 && code != 2 {
            return code;
        }
        if axis == 0 {
            // A scalar array-element designator accepts one value. Expanded
            // element reads are a nonstandard extension in other compilers.
            if !self.new_assignment && self.rank!=0 {return 4;}
            self.apply = self.new_assignment && self.rank != 0;
            self.new_assignment = false;
            self.stride = 1;
            if self.apply {
                if self.rank as i64 != rank {
                    return 7;
                }
                *index = 1;
            }
        } else if self.apply {
            let Some(sub) = axis
                .checked_sub(1)
                .and_then(|a| self.subscripts.get(a as usize))
            else {
                return 7;
            };
            if *sub < lower || *sub > upper {
                return 4;
            }
            let result = sub
                .checked_sub(lower)
                .and_then(|d| d.checked_mul(self.stride))
                .and_then(|d| index.checked_add(d));
            let Some(result) = result else {
                return 4;
            };
            *index = result;
            if axis == rank {
                self.index = *index;
                self.apply = false;
            } else {
                let Some(extent) = upper
                    .checked_sub(lower)
                    .and_then(|d| d.checked_add(1))
                    .and_then(|d| d.checked_mul(self.stride))
                else {
                    return 4;
                };
                self.stride = extent;
            }
        }
        code
    }
    pub(crate) fn next(
        &mut self,
        field: &mut [u8],
        token: &mut [u8],
        index: &mut i64,
        mut read: impl FnMut() -> Result<Option<u8>, ()>,
    ) -> i64 {
        let result = self.event(&mut read).unwrap_or_else(|e| e);
        field.fill(b' ');
        token.fill(b' ');
        *index = self.index;
        if self.field_len > field.len() || self.length > token.len() {
            return 6;
        }
        field[..self.field_len].copy_from_slice(&self.field[..self.field_len]);
        token[..self.length].copy_from_slice(&self.token[..self.length]);
        result
    }
}

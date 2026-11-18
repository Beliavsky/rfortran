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
    /// Values per element when subscripts select a whole element of an array
    /// of derived type, and the last value index of the selected element.
    scale: i64,
    element_end: i64,
    /// Subscripts of the element of an array of derived type in a designator
    /// such as `x(2)%a`, and the stride while linearizing them.
    outer: [i64; 15],
    outer_rank: usize,
    outer_stride: i64,
    /// Subscript triplets `[lo]:[hi][:step]` of an array section designator:
    /// per axis, whether it is a triplet, its upper bound (if given), its
    /// step, and whether the lower bound was given. Values then define the
    /// section's elements in array element order.
    triplet: [bool; 15],
    upper: [Option<i64>; 15],
    step: [i64; 15],
    given: [bool; 15],
    section: bool,
    /// While mapping a value of a section: the element ordinal still to be
    /// distributed over the axes, the linear index and its stride.
    remaining: i64,
    linear: i64,
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
            scale: 1,
            element_end: 0,
            outer: [0; 15],
            outer_rank: 0,
            outer_stride: 1,
            triplet: [false; 15],
            upper: [None; 15],
            step: [1; 15],
            given: [false; 15],
            section: false,
            remaining: 0,
            linear: 1,
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
            // A group starts with `&name`, or `$name` as in other compilers.
            if c != b'&' && c != b'$' {
                continue;
            }
            'group: loop {
                self.marker_name(read)?;
                if self.token[..self.length] == self.group[..self.group_len] {
                    self.found = true;
                    return Ok(());
                }
                // Skip a different group, honoring strings and comments, to
                // its `/`, `&END` or `$END`; another group may start there.
                loop {
                    self.spaces(read)?;
                    let c = self.get(read)?.ok_or(-1)?;
                    if c == b'/' {
                        break 'group;
                    }
                    if c == b'&' || c == b'$' {
                        self.marker_name(read)?;
                        if &self.token[..self.length] == b"end" {
                            break 'group;
                        }
                        // The name is that of the next group.
                        if self.token[..self.length] == self.group[..self.group_len] {
                            self.found = true;
                            return Ok(());
                        }
                        continue;
                    }
                    if c == b'\'' || c == b'"' {
                        self.length = 0;
                        self.quoted(c, read)?;
                    }
                }
            }
        }
    }
    /// The name after `&` or `$`, in lower case, as the token.
    fn marker_name(&mut self, read: &mut impl FnMut() -> Result<Option<u8>, ()>) -> Result<(), i64> {
        self.length = 0;
        while let Some(c) = self.peek(read)? {
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.get(read)?;
                self.push(c.to_ascii_lowercase())?;
            } else {
                break;
            }
        }
        Ok(())
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
            // `&END` or `$END` also ends a group, as in other compilers.
            if c == b'&' || c == b'$' {
                self.marker_name(read)?;
                if &self.token[..self.length] != b"end" {
                    return Err(3);
                }
                self.length = 0;
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
                // A NaN may carry a payload, as NaN(0x33); its value is a NaN.
                if self.token[..self.length].eq_ignore_ascii_case(b"nan") && self.peek(read)? == Some(b'(') {
                    loop {
                        match self.get(read)? {
                            Some(b')') => break,
                            Some(c) if c.is_ascii_alphanumeric() || c == b'(' || c == b'_' => {}
                            _ => return Err(1),
                        }
                    }
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
                            .any(|c| !c.is_ascii_alphanumeric() && !matches!(*c, b'_' | b'%'))
                    {
                        return Err(3);
                    }
                    self.field_len = self.length;
                    for j in 0..self.length {
                        self.field[j] = self.token[j].to_ascii_lowercase();
                    }
                    // Subscripts followed by `%` select an element of an array
                    // of derived type. They are kept as the outer subscripts
                    // and the field becomes `x()%a`; only one array of derived
                    // type may be subscripted. The final subscripts address
                    // the object.
                    self.outer_rank = 0;
                    self.section = false;
                    loop {
                        self.rank = 0;
                        if self.peek(read)? == Some(b'(') {
                            self.get(read)?;
                            loop {
                                if self.rank == 15 {
                                    return Err(7);
                                }
                                let k = self.rank;
                                let first = self.integer(read)?;
                                self.triplet[k] = false;
                                self.given[k] = first.is_some();
                                self.subscripts[k] = first.unwrap_or(0);
                                self.upper[k] = None;
                                self.step[k] = 1;
                                if self.peek(read)? == Some(b':') {
                                    self.get(read)?;
                                    self.triplet[k] = true;
                                    self.section = true;
                                    self.upper[k] = self.integer(read)?;
                                    if self.peek(read)? == Some(b':') {
                                        self.get(read)?;
                                        self.step[k] = self.integer(read)?.ok_or(7)?;
                                        if self.step[k] == 0 {
                                            return Err(7);
                                        }
                                    }
                                } else if first.is_none() {
                                    return Err(7);
                                }
                                self.rank += 1;
                                match self.get(read)? {
                                    Some(b',') => continue,
                                    Some(b')') => break,
                                    _ => return Err(7),
                                }
                            }
                            self.spaces(read)?;
                        }
                        if self.rank == 0 || self.peek(read)? != Some(b'%') {
                            break;
                        }
                        self.get(read)?;
                        if self.outer_rank != 0 || self.section {
                            return Err(7);
                        }
                        self.outer[..self.rank].copy_from_slice(&self.subscripts[..self.rank]);
                        self.outer_rank = self.rank;
                        let mut text = [0u8; 128];
                        let mut n = 0;
                        for c in *b"()%" {
                            text[n] = c;
                            n += 1;
                        }
                        while let Some(c) = self.peek(read)? {
                            if !c.is_ascii_alphanumeric() && !matches!(c, b'_' | b'%') {
                                break;
                            }
                            self.get(read)?;
                            if n == text.len() {
                                return Err(3);
                            }
                            text[n] = c.to_ascii_lowercase();
                            n += 1;
                        }
                        if self.field_len + n > self.field.len() {
                            return Err(3);
                        }
                        self.field[self.field_len..self.field_len + n].copy_from_slice(&text[..n]);
                        self.field_len += n;
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
        // A negative axis linearizes the outer subscripts into `index`.
        if axis < 0 {
            let k = (-axis - 1) as usize;
            if self.outer_rank as i64 != rank || k >= self.outer_rank {
                return 7;
            }
            if k == 0 {
                *index = 1;
                self.outer_stride = 1;
            }
            let sub = self.outer[k];
            if sub < lower || sub > upper {
                return 4;
            }
            let Some(result) = sub.checked_sub(lower).and_then(|d| d.checked_mul(self.outer_stride)).and_then(|d| index.checked_add(d)) else {
                return 4;
            };
            *index = result;
            let Some(stride) = upper.checked_sub(lower).and_then(|d| d.checked_add(1)).and_then(|d| d.checked_mul(self.outer_stride)) else {
                return 4;
            };
            self.outer_stride = stride;
            return code;
        }
        if self.section && axis >= 0 {
            return self.section_layout(index, code, axis, rank, lower, upper);
        }
        if axis == 0 {
            // Values after the first of an array-element designator define
            // the following elements in array element order, as in gfortran
            // and other compilers (an extension). A whole element of an array
            // of derived type accepts the values of its components (`lower`
            // is the number of values per element).
            if !self.new_assignment && self.rank!=0 && self.scale > 1 && self.index > self.element_end {return 4;}
            if self.new_assignment {self.scale = lower.max(1);}
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
                if self.scale > 1 {
                    let Some(start) = index.checked_sub(1).and_then(|e| e.checked_mul(self.scale)).and_then(|e| e.checked_add(1)) else {
                        return 4;
                    };
                    *index = start;
                    self.element_end = start + self.scale - 1;
                }
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
    /// An optionally signed integer, after optional blanks; `None` if there
    /// are no digits.
    fn integer(&mut self, read: &mut impl FnMut() -> Result<Option<u8>, ()>) -> Result<Option<i64>, i64> {
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
        self.spaces(read)?;
        if self.length == 0 {
            return Ok(None);
        }
        let n = core::str::from_utf8(&self.token[..self.length])
            .map_err(|_| 7)?
            .parse::<i64>()
            .map_err(|_| 7)?;
        self.length = 0;
        Ok(Some(n))
    }
    /// Map the current value of a section designator to its linear index.
    /// The value ordinal (`self.index`) selects an element of the section, in
    /// array element order; a whole element of an array of derived type
    /// takes `scale` values.
    fn section_layout(&mut self, index: &mut i64, code: i64, axis: i64, rank: i64, lower: i64, upper: i64) -> i64 {
        if axis == 0 {
            if self.new_assignment {
                self.scale = lower.max(1);
            }
            self.new_assignment = false;
            if self.rank as i64 != rank {
                return 7;
            }
            let ordinal = self.index - 1;
            self.remaining = ordinal / self.scale;
            self.linear = 1;
            self.stride = 1;
            return code;
        }
        let k = (axis - 1) as usize;
        if k >= self.rank {
            return 7;
        }
        let sub = if self.triplet[k] {
            // Omitted bounds are the declared bounds, whatever the step.
            let lo = if self.given[k] { self.subscripts[k] } else { lower };
            let hi = self.upper[k].unwrap_or(upper);
            let n = ((hi - lo) / self.step[k] + 1).max(0);
            if n == 0 {
                return 4;
            }
            let last = lo + (n - 1) * self.step[k];
            if lo.min(last) < lower || lo.max(last) > upper {
                return 4;
            }
            let sub = lo + (self.remaining % n) * self.step[k];
            self.remaining /= n;
            sub
        } else {
            self.subscripts[k]
        };
        if sub < lower || sub > upper {
            return 4;
        }
        self.linear += (sub - lower) * self.stride;
        self.stride *= upper - lower + 1;
        if axis == rank {
            if self.remaining > 0 {
                return 4;
            }
            let within = (self.index - 1) % self.scale;
            *index = (self.linear - 1) * self.scale + within + 1;
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

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Records of unformatted files, shared by the interpreter and the native
//! runtime. A sequential record is laid out as gfortran writes it: its length
//! in bytes as a 4-byte little-endian integer, the data, then the length
//! again, so that BACKSPACE can step back over it. A direct-access record is
//! RECL bytes at offset (REC-1)*RECL, with no markers. A stream file has no
//! records.

// Sequential access, the default, is 0.
pub const STREAM: u8 = 1;
pub const DIRECT: u8 = 2;

/// IOSTAT for end of file.
pub const END: i64 = -1;
/// IOSTAT for an error, as a transfer past the end of a record.
pub const ERROR: i64 = 1;

/// How a unit is connected, from its OPEN statement.
#[derive(Clone, Copy, Default)]
pub struct Unit {
    pub access: u8,
    /// FORM='UNFORMATTED', or an unformatted transfer has been made.
    pub unformatted: bool,
    pub recl: i64,
    /// A READ found the end of the file, so the unit is after its endfile
    /// record, which a BACKSPACE steps back over.
    pub after_end: bool,
}

/// The record that an unformatted READ or WRITE is transferring.
#[derive(Clone, Copy)]
pub struct Active {
    pub writing: bool,
    pub direct: bool,
    /// Where the record starts: its leading length for a sequential record.
    pub start: i64,
    /// Where its data ends, which a transfer may not pass.
    pub end: i64,
    /// Transfers within this one, as by a defined input/output procedure,
    /// which continue its record.
    pub depth: u32,
}

/// The file operations that records need.
pub trait Io {
    fn tell(&mut self) -> Option<i64>;
    fn seek(&mut self, position: i64) -> bool;
    /// The number of bytes read.
    fn read(&mut self, buffer: &mut [u8]) -> usize;
    fn write(&mut self, bytes: &[u8]) -> bool;
    /// End the file at the current position.
    fn truncate(&mut self) -> bool;
    fn length(&mut self) -> Option<i64>;
}

/// Start a record: REC is the record number of a direct-access transfer, or
/// zero. Returns the IOSTAT code and, for a record, its extent.
pub fn begin(unit: &mut Unit, io: &mut dyn Io, rec: i64, writing: bool) -> (i64, Option<Active>) {
    match unit.access {
        STREAM => (if rec != 0 { ERROR } else { 0 }, None),
        DIRECT => {
            if rec < 1 || unit.recl < 1 { return (ERROR, None); }
            let Some(start) = (rec - 1).checked_mul(unit.recl) else { return (ERROR, None) };
            // A record that was never written cannot be read.
            if !writing && io.length().is_none_or(|length| start + unit.recl > length) { return (ERROR, None); }
            if !io.seek(start) { return (ERROR, None); }
            (0, Some(Active { writing, direct: true, start, end: start + unit.recl, depth: 0 }))
        }
        _ => {
            if rec != 0 { return (ERROR, None); }
            unit.unformatted = true;
            unit.after_end = false;
            let Some(start) = io.tell() else { return (ERROR, None) };
            if writing {
                if !io.write(&[0; 4]) { return (ERROR, None); }
                // RECL, when given, limits the length of a record.
                let end = if unit.recl > 0 { start + 4 + unit.recl } else { i64::MAX };
                return (0, Some(Active { writing, direct: false, start, end, depth: 0 }));
            }
            let mut marker = [0; 4];
            match io.read(&mut marker) {
                0 => { unit.after_end = true; (END, None) }
                4 => {
                    let length = i32::from_le_bytes(marker);
                    if length < 0 { return (ERROR, None); }
                    (0, Some(Active { writing, direct: false, start, end: start + 4 + i64::from(length), depth: 0 }))
                }
                _ => (ERROR, None),
            }
        }
    }
}

/// Whether `width` more bytes fit in the record.
pub fn fits(active: &Active, io: &mut dyn Io, width: usize) -> bool {
    io.tell().is_some_and(|position| position + width as i64 <= active.end)
}

/// Finish a record: write its lengths, or pad it to RECL, or skip the rest of
/// it. Returns the IOSTAT code.
pub fn end(active: &Active, io: &mut dyn Io) -> i64 {
    let okay = match (active.direct, active.writing) {
        (true, true) => match io.tell() {
            Some(position) if position <= active.end => {
                let mut ok = true;
                for _ in position..active.end { ok &= io.write(&[0]); }
                ok
            }
            _ => false,
        },
        (true, false) => io.seek(active.end),
        (false, true) => match io.tell() {
            Some(position) => {
                let length = position - active.start - 4;
                match i32::try_from(length) {
                    Ok(length) => {
                        let marker = length.to_le_bytes();
                        // A sequential WRITE ends the file after its record.
                        io.write(&marker) && io.seek(active.start) && io.write(&marker) && io.seek(position + 4) && io.truncate()
                    }
                    Err(_) => false,
                }
            }
            None => false,
        },
        (false, false) => io.seek(active.end + 4),
    };
    if okay { 0 } else { ERROR }
}

/// BACKSPACE over an unformatted sequential record; None when the unit is
/// not unformatted sequential.
pub fn backspace(unit: &mut Unit, io: &mut dyn Io) -> Option<i64> {
    if unit.access == DIRECT { return Some(ERROR); }
    if unit.access == STREAM { return None; }
    if !unit.unformatted { return None; }
    if unit.after_end { unit.after_end = false; return Some(0); }
    let position = io.tell()?;
    if position < 4 { return Some(if io.seek(0) { 0 } else { ERROR }); }
    let mut marker = [0; 4];
    if !io.seek(position - 4) || io.read(&mut marker) != 4 { return Some(ERROR); }
    let start = position - 8 - i64::from(i32::from_le_bytes(marker));
    Some(if start >= 0 && io.seek(start) { 0 } else { ERROR })
}

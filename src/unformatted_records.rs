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
    /// The record after the last one transferred by direct access, less one
    /// (so zero, the default, means record 1), for INQUIRE NEXTREC.
    pub last_record: i64,
    /// OPEN's SIGN=, DELIM= and PAD= for formatted transfers, as the bits
    /// SIGN_PLUS through PAD_NO.
    pub mode: u8,
}

/// Bits of `Unit::mode`. SIGN='PLUS' starts each formatted output statement
/// as SP does; SIGN='SUPPRESS' is the default and is kept only for INQUIRE.
pub const SIGN_PLUS: u8 = 1;
pub const SIGN_SUPPRESS: u8 = 2;
/// DELIM='APOSTROPHE' or 'QUOTE' delimits CHARACTER values in list-directed
/// output.
pub const DELIM_APOSTROPHE: u8 = 4;
pub const DELIM_QUOTE: u8 = 8;
/// PAD='NO': a formatted READ that needs more characters than its record
/// holds ends with IOSTAT END_OF_RECORD instead of reading blanks.
pub const PAD_NO: u8 = 16;
/// DELIM='NONE' given explicitly: NAMELIST output, which otherwise uses
/// quotes, writes CHARACTER values undelimited.
pub const DELIM_NONE: u8 = 32;

impl Unit {
    pub fn new(access: u8, unformatted: bool, recl: i64, mode: u8) -> Self {
        Unit { access, unformatted, recl, after_end: false, last_record: 0, mode }
    }
    fn formatted(&self) -> bool { !self.unformatted && self.access != DIRECT }
}

/// What INQUIRE asks of a connection.
pub const ACCESS: i64 = 0;
pub const RECL: i64 = 1;
pub const NEXTREC: i64 = 2;
pub const POS: i64 = 3;
pub const FORM: i64 = 4;
/// 0 PROCESSOR_DEFINED, 1 PLUS, 2 SUPPRESS.
pub const SIGN: i64 = 5;
/// 0 NONE, 1 APOSTROPHE, 2 QUOTE.
pub const DELIM: i64 = 6;
/// 0 YES, 1 NO.
pub const PAD: i64 = 7;
/// The delimiter of NAMELIST CHARACTER output: 0 none, 1 apostrophe, 2
/// quote (the default).
pub const NAMELIST_DELIM: i64 = 8;
/// The RECL that gfortran reports for a sequential connection without one.
pub const DEFAULT_RECL: i64 = 1073741824;

/// INQUIRE of standard input, output or error, preconnected for formatted
/// sequential access.
pub fn preconnected(which: i64) -> i64 {
    match which {
        ACCESS | FORM | SIGN | DELIM | PAD => 0,
        NAMELIST_DELIM => 2,
        RECL => DEFAULT_RECL,
        _ => -1,
    }
}

/// The answer to INQUIRE `which` for a unit, or -1 when it is undefined;
/// `connected` tells whether a file is connected, and `position` is the file
/// position in bytes from 0. ACCESS is 0 sequential, 1 stream, 2 direct; FORM
/// is 0 formatted, 1 unformatted.
pub fn inquire(unit: &Unit, connected: bool, position: Option<i64>, which: i64) -> i64 {
    if !connected { return -1; }
    match which {
        ACCESS => i64::from(unit.access),
        RECL => if unit.recl > 0 { unit.recl } else if unit.access == STREAM { -1 } else { DEFAULT_RECL },
        NEXTREC => if unit.access == DIRECT { unit.last_record + 1 } else { -1 },
        POS => if unit.access == STREAM { position.map_or(-1, |p| p + 1) } else { -1 },
        FORM => i64::from(unit.unformatted || unit.access == DIRECT),
        // DELIM and PAD describe formatted connections only; gfortran gives
        // PROCESSOR_DEFINED for the SIGN of an unformatted one.
        DELIM | PAD if !unit.formatted() => -1,
        SIGN => if unit.mode & SIGN_PLUS != 0 { 1 } else if unit.mode & SIGN_SUPPRESS != 0 { 2 } else { 0 },
        DELIM => if unit.mode & DELIM_APOSTROPHE != 0 { 1 } else if unit.mode & DELIM_QUOTE != 0 { 2 } else { 0 },
        PAD => i64::from(unit.mode & PAD_NO != 0),
        NAMELIST_DELIM => if unit.mode & DELIM_NONE != 0 { 0 } else if unit.mode & DELIM_APOSTROPHE != 0 { 1 } else { 2 },
        _ => -1,
    }
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
            unit.last_record = rec;
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

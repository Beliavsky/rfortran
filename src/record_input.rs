// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Shared, allocation-free (A) record scanner for interpreter and native I/O.
pub(crate) const END: i64 = -1;
pub(crate) const ERROR: i64 = 1;
pub(crate) const INVALID: i64 = 2;

pub(crate) fn record(
    destination: &mut [u8],
    mut byte: impl FnMut() -> Result<Option<u8>, ()>,
) -> i64 {
    // With no characters transferred, discard the record without interpreting
    // its bytes. This is also used by an empty list-directed READ.
    if destination.is_empty() {
        let mut consumed = false;
        loop {
            match byte() {
                Ok(Some(b'\n')) => return 0,
                Ok(Some(_)) => consumed = true,
                Ok(None) => return if consumed { 0 } else { END },
                Err(()) => return ERROR,
            }
        }
    }
    destination.fill(b' ');
    let mut count = 0usize;
    let mut previous = 0u8;
    let mut invalid = false;
    loop {
        let value = match byte() {
            Ok(Some(value)) => value,
            Ok(None) => return if invalid { INVALID } else if count == 0 { END } else { 0 },
            Err(()) => return ERROR,
        };
        if value == b'\n' {
            if previous == b'\r' && count > 0 && count <= destination.len() {
                destination[count-1] = b' ';
            }
            return if invalid { INVALID } else { 0 };
        }
        invalid |= value == 0 || value > 127;
        if count < destination.len() { destination[count] = value; }
        count = count.saturating_add(1).min(destination.len().saturating_add(1));
        previous = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_transfers_skip_arbitrary_records_without_losing_eof() {
        let mut input = b"\xff\0\xc3\xa9\r\n\nlast".iter().copied();
        for _ in 0..3 { assert_eq!(record(&mut [], || Ok(input.next())), 0); }
        assert_eq!(record(&mut [], || Ok(input.next())), END);
        assert_eq!(record(&mut [], || Err(())), ERROR);
    }

    #[test]
    fn consumes_records_and_distinguishes_eof_from_errors() {
        let mut input = b"abcdef\r\n\nlast".iter().copied();
        let mut text = [0; 3];
        assert_eq!(record(&mut text, || Ok(input.next())), 0);
        assert_eq!(&text, b"abc");
        assert_eq!(record(&mut text, || Ok(input.next())), 0);
        assert_eq!(&text, b"   ");
        assert_eq!(record(&mut text, || Ok(input.next())), 0);
        assert_eq!(&text, b"las");
        assert_eq!(record(&mut text, || Ok(input.next())), END);
        assert_eq!(record(&mut text, || Err(())), ERROR);
        let mut input = b"x\0yz\nokay\n".iter().copied();
        assert_eq!(record(&mut text, || Ok(input.next())), INVALID);
        assert_eq!(record(&mut text, || Ok(input.next())), 0);
        assert_eq!(&text, b"oka");
        let mut input = b"a\r\nx\n".iter().copied();
        assert_eq!(record(&mut text, || Ok(input.next())), 0);
        assert_eq!(&text, b"a  ");
        assert_eq!(record(&mut [], || Ok(input.next())), 0);
        assert_eq!(record(&mut [], || Ok(input.next())), END);
    }
}

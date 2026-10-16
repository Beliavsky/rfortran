// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! One scalar CHARACTER field from an internal list-directed record.
//! 0: assigned, 1: null/termination (unchanged), -1: end of record, 2: bad input.
/// Fortran CHARACTER comparison pads the shorter value with blanks.
pub(crate) fn compare(left:&[u8],right:&[u8])->i32 {
    for i in 0..left.len().max(right.len()) {
        let a=left.get(i).copied().unwrap_or(b' ');let b=right.get(i).copied().unwrap_or(b' ');
        if a!=b {return if a<b {-1}else{1};}
    }0
}

pub(crate) fn field(source: &[u8], destination: &mut [u8]) -> i64 {
    let mut i = 0;
    while source
        .get(i)
        .is_some_and(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n'))
    {
        i += 1;
    }
    let Some(&first) = source.get(i) else {
        return -1;
    };
    if matches!(first, b',' | b'/') {
        return 1;
    }
    // Repeated values supply the same first value; a repeated null leaves it alone.
    let begin = i;
    while source.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    if i > begin && source.get(i) == Some(&b'*') {
        if source[begin..i].iter().all(|c| *c == b'0') {
            return 2;
        }
        i += 1;
        if source
            .get(i)
            .is_none_or(|c| matches!(c, b' ' | b'\t' | b',' | b'/'))
        {
            return 1;
        }
    } else {
        i = begin;
    }
    let quote = source[i];
    let quoted = matches!(quote, b'\'' | b'"');
    if quoted {
        i += 1;
    }
    destination.fill(b' ');
    let mut count = 0;
    loop {
        let Some(&ch) = source.get(i) else {
            return if quoted { -1 } else { 0 };
        };
        if ch == 0 || ch > 127 {
            return 2;
        }
        if quoted && ch == quote {
            i += 1;
            if source.get(i) != Some(&quote) {
                return if source
                    .get(i)
                    .is_none_or(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n' | b',' | b'/'))
                {
                    0
                } else {
                    2
                };
            }
        } else if !quoted && matches!(ch, b' ' | b'\t' | b'\r' | b'\n' | b',' | b'/') {
            return 0;
        }
        if count < destination.len() {
            destination[count] = ch;
        }
        count += 1;
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::field;
    #[test]
    fn character_fields() {
        for (input, code, value) in [
            (" ! first", 0, "!    "),
            ("!", 0, "!    "),
            ("'a b'", 0, "a b  "),
            ("\"a\"\"b\"", 0, "a\"b  "),
            ("3*'xy'", 0, "xy   "),
            ("2*", 1, "old  "),
            (",", 1, "old  "),
            ("/", 1, "old  "),
            ("   ", -1, "old  "),
            ("'oops", -1, "oops "),
            ("'a'b", 2, "a    "),
            ("abcdef", 0, "abcde"),
        ] {
            let mut out = *b"old  ";
            assert_eq!(field(input.as_bytes(), &mut out), code, "{input}");
            assert_eq!(&out, value.as_bytes(), "{input}");
        }
    }
}

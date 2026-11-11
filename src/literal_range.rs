// SPDX-License-Identifier: MIT
//! Reject integer literals too big for their kind.
//!
//! A literal without a kind parameter, such as 9223372036854775807, is a
//! default INTEGER, and the standard requires its value to be representable in
//! that kind. A minus sign is an operator, not part of the literal, so
//! -2147483648 is also invalid. gfortran reports both. A kind-suffixed literal
//! such as 9223372036854775807_8 is checked against its own kind later.
use crate::lexer::{Token, TokenKind};
use anyhow::Result;

pub(crate) fn check(tokens: &[Token]) -> Result<()> {
    for token in tokens {
        let TokenKind::Integer(text) = &token.kind else { continue };
        // Kind-suffixed and BOZ literals contain '_'; labels are short.
        if !text.bytes().all(|b| b.is_ascii_digit()) { continue; }
        let digits = text.trim_start_matches('0');
        let too_big = digits.len() > 10 || digits.parse::<u64>().is_ok_and(|v| v > i32::MAX as u64);
        if too_big {
            return Err(crate::source_diagnostics::at(
                anyhow::anyhow!(
                    "integer literal {text} is too big for default INTEGER (largest {}); \
                     give it a kind, such as {text}_int64 with USE ISO_FORTRAN_ENV, or {text}_8",
                    i32::MAX
                ),
                Some(token.span.start),
            ));
        }
    }
    Ok(())
}

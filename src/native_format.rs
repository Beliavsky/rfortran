// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Built once with the compiler and embedded as a native static library.
#![no_std]
mod format_spec;
use format_spec::Parser;

extern "C" {
    fn printf(format: *const u8, ...) -> i32;
    fn sprintf(buffer: *mut u8, format: *const u8, ...) -> i32;
    fn putchar(c: i32) -> i32;
    fn exit(code: i32) -> !;
    #[cfg(windows)]
    fn _write(fd: i32, data: *const u8, len: u32) -> i32;
    #[cfg(not(windows))]
    fn write(fd: i32, data: *const u8, len: usize) -> isize;
}
fn fail(message: &str) -> ! {
    unsafe {
        for text in ["Runtime error: invalid output format: ", message, "\n"] {
            #[cfg(windows)]
            _write(2, text.as_ptr(), text.len() as u32);
            #[cfg(not(windows))]
            write(2, text.as_ptr(), text.len());
        }
        exit(1)
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    fail("internal formatting failure")
}

// Fixed 24-byte layout also used by the Cranelift call site.
#[repr(C)]
pub struct Item {
    tag: u64,
    bits: u64,
    length: u64,
}
fn checked<T>(result: Result<T, &'static str>) -> T {
    match result {
        Ok(v) => v,
        Err(e) => fail(e),
    }
}
/// The compiler supplies valid format bytes and an aligned array of `count`
/// Items. CHARACTER items contain a valid pointer and length; other items hold
/// INTEGER, REAL, or LOGICAL bits. All storage remains live for this call.
#[no_mangle]
pub unsafe extern "C" fn rfort_formatted_output(
    format: *const u8,
    length: usize,
    items: *const Item,
    count: usize,
) {
    let bytes = core::slice::from_raw_parts(format, length);
    let items = core::slice::from_raw_parts(items, count);
    // Validate the entire statement before writing any output.
    let mut parser = checked(Parser::new(bytes));
    let mut index = 0;
    while let Some(d) = checked(parser.next()) {
        if matches!(d.code, b'a' | b'i' | b'f' | b'l') {
            for _ in 0..d.repeat {
                let Some(item) = items.get(index) else {
                    fail("format descriptor count must match the output list; format reversion is not yet supported");
                };
                if !format_spec::matches_type(d.code, item.tag) {
                    fail("descriptor does not match output item type");
                }
                index += 1;
            }
        }
    }
    if index != count {
        fail("format descriptor count must match the output list; format reversion is not yet supported");
    }
    let mut parser = checked(Parser::new(bytes));
    index = 0;
    while let Some(d) = checked(parser.next()) {
        for _ in 0..d.repeat {
            match d.code {
                b'\'' | b'"' => {
                    let mut i = 0;
                    while i < d.literal.len() {
                        let c = d.literal[i];
                        putchar(c as i32);
                        i += 1;
                        if c == d.code && d.literal.get(i) == Some(&c) {
                            i += 1;
                        }
                    }
                }
                b'x' => {
                    putchar(32);
                }
                b'/' => {
                    putchar(10);
                }
                _ => {
                    let item = &items[index];
                    index += 1;
                    if d.code == b'a' {
                        let n = if d.width == 0 {
                            item.length as usize
                        } else {
                            (item.length as usize).min(d.width)
                        };
                        for _ in n..d.width {
                            putchar(32);
                        }
                        printf(
                            b"%.*s\0".as_ptr(),
                            n as i32,
                            item.bits as usize as *const u8,
                        );
                    } else {
                        // Maximum F precision is 256; even DBL_MAX needs fewer than 600 bytes.
                        let mut buffer = [0u8; 1024];
                        let n = match d.code {
                            b'i' => sprintf(
                                buffer.as_mut_ptr(),
                                b"%.*lld\0".as_ptr(),
                                d.precision as i32,
                                item.bits as i64,
                            ),
                            b'f' => sprintf(
                                buffer.as_mut_ptr(),
                                b"%.*f\0".as_ptr(),
                                d.precision as i32,
                                f64::from_bits(item.bits),
                            ),
                            _ => {
                                buffer[0] = if item.bits == 0 { b'F' } else { b'T' };
                                1
                            }
                        };
                        if n < 0 || n as usize >= buffer.len() {
                            fail("numeric formatting failed");
                        }
                        let n = n as usize;
                        if d.width != 0 && n > d.width {
                            for _ in 0..d.width {
                                putchar(b'*' as i32);
                            }
                        } else {
                            for _ in n..d.width {
                                putchar(32);
                            }
                            if d.width == 0 && n == 0 && d.code == b'i' {
                                putchar(32);
                            }
                            printf(b"%s\0".as_ptr(), buffer.as_ptr());
                        }
                    }
                }
            }
        }
    }
    putchar(10);
}

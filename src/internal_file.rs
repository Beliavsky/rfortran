// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Statement-scoped character files for defined I/O. Units are reserved by the
//! caller in the ordinary unit table; only this module owns their byte storage.
use core::{ptr, slice};
extern "C" {
    fn malloc(n: usize) -> *mut core::ffi::c_void;
    fn free(p: *mut core::ffi::c_void);
}
struct File {
    unit: i64,
    mode: i64,
    bytes: *mut u8,
    length: usize,
    width: usize,
    records: usize,
    record: usize,
    column: usize,
    next: *mut File,
}
#[cfg(native_runtime)]
static mut FILES: *mut File = ptr::null_mut();
#[cfg(not(native_runtime))]
std::thread_local! {static FILES:core::cell::Cell<*mut File>=const {core::cell::Cell::new(ptr::null_mut())};}
unsafe fn head() -> *mut File {
    #[cfg(native_runtime)]
    {
        FILES
    }
    #[cfg(not(native_runtime))]
    {
        FILES.with(core::cell::Cell::get)
    }
}
unsafe fn set_head(p: *mut File) {
    #[cfg(native_runtime)]
    {
        FILES = p;
    }
    #[cfg(not(native_runtime))]
    {
        FILES.with(|files| files.set(p));
    }
}
unsafe fn find(unit: i64) -> *mut File {
    let mut p = head();
    while !p.is_null() {
        if (*p).unit == unit {
            return p;
        }
        p = (*p).next;
    }
    ptr::null_mut()
}
pub unsafe fn mode(unit: i64) -> i64 {
    let p = find(unit);
    if p.is_null() {
        0
    } else {
        (*p).mode
    }
}
pub unsafe fn width(unit:i64)->Option<usize> {let p=find(unit);if p.is_null(){None}else{Some((*p).width)}}
pub unsafe fn begin(unit: i64, source: &[u8], width: usize, records: usize, writing: bool) -> i64 {
    if mode(unit) != 0 || width.checked_mul(records) != Some(source.len()) {
        return 1;
    }
    let code = allocate(unit, width, records, writing);
    if code == 0 && !writing {
        ptr::copy_nonoverlapping(source.as_ptr(), (*find(unit)).bytes, source.len());
    }
    code
}
pub unsafe fn allocate(unit: i64, width: usize, records: usize, writing: bool) -> i64 {
    if mode(unit) != 0 {
        return 1;
    }
    let Some(length) = width.checked_mul(records) else {
        return 1;
    };
    let bytes = malloc(length.max(1)).cast::<u8>();
    if bytes.is_null() {
        return 1;
    }
    ptr::write_bytes(bytes, b' ', length);
    let p = malloc(core::mem::size_of::<File>()).cast::<File>();
    if p.is_null() {
        free(bytes.cast());
        return 1;
    }
    ptr::write(
        p,
        File {
            unit,
            mode: if writing { 2 } else { 1 },
            bytes,
            length,
            width,
            records,
            record: 0,
            column: 0,
            next: head(),
        },
    );
    set_head(p);
    0
}
/// Load an input record, or retrieve a reached output record. Unreached records
/// return 2 so the caller leaves the corresponding array element untouched.
pub unsafe fn record(unit: i64, index: i64, bytes: &mut [u8], loading: bool) -> i64 {
    let p = find(unit);
    if p.is_null() || index < 1 {
        return 1;
    }
    let f = &mut *p;
    let r = index as usize - 1;
    if r >= f.records || bytes.len() != f.width {
        return 1;
    }
    if !loading && r > f.record {
        return 2;
    }
    let storage = slice::from_raw_parts_mut(f.bytes.add(r * f.width), f.width);
    if loading {
        storage.copy_from_slice(bytes);
    } else {
        bytes.copy_from_slice(storage);
    }
    0
}
pub unsafe fn end(unit: i64, destination: &mut [u8], writing: bool) -> i64 {
    let mut previous: *mut File = ptr::null_mut();
    let mut p = head();
    while !p.is_null() {
        if (*p).unit == unit {
            let f = &*p;
            let code = if writing && destination.len() != f.length {
                1
            } else {
                if writing {
                    destination.copy_from_slice(slice::from_raw_parts(f.bytes, f.length));
                }
                0
            };
            if previous.is_null() {
                set_head(f.next);
            } else {
                (*previous).next = f.next;
            }
            free(f.bytes.cast());
            free(p.cast());
            return code;
        }
        previous = p;
        p = (*p).next;
    }
    1
}
pub unsafe fn write(unit: i64, byte: u8) -> i64 {
    let p = find(unit);
    if p.is_null() || (*p).mode != 2 {
        return 1;
    }
    let f = &mut *p;
    if byte == b'\n' {
        if f.record + 1 >= f.records {
            return -2;
        }
        f.record += 1;
        f.column = 0;
        return 0;
    }
    if f.record >= f.records || f.column >= f.width {
        return -2;
    }
    *f.bytes.add(f.record * f.width + f.column) = byte;
    f.column += 1;
    0
}
pub unsafe fn byte(unit: i64) -> Result<Option<u8>, ()> {
    let p = find(unit);
    if p.is_null() || (*p).mode != 1 {
        return Err(());
    }
    let f = &mut *p;
    if f.record >= f.records {
        return Ok(None);
    }
    if f.column == f.width {
        f.record += 1;
        f.column = 0;
        return Ok(Some(b'\n'));
    }
    let b = *f.bytes.add(f.record * f.width + f.column);
    f.column += 1;
    Ok(Some(b))
}
/// A private snapshot with record delimiters for the native list scanner.
/// The caller frees it using the same C allocator.
#[cfg(native_runtime)]
pub unsafe fn input_copy(unit: i64) -> Option<(*mut u8, usize)> {
    let p = find(unit);
    if p.is_null() || (*p).mode != 1 {
        return None;
    }
    let f = &*p;
    let length = f.length.checked_add(f.records)?;
    let data = malloc(length.max(1)).cast::<u8>();
    if data.is_null() {
        return None;
    }
    for r in 0..f.records {
        ptr::copy_nonoverlapping(
            f.bytes.add(r * f.width),
            data.add(r * (f.width + 1)),
            f.width,
        );
        *data.add(r * (f.width + 1) + f.width) = b'\n';
    }
    Some((data, length))
}

#[cfg(not(native_runtime))]
pub unsafe fn clear() {
    while !head().is_null() {
        let p = head();
        set_head((*p).next);
        free((*p).bytes.cast());
        free(p.cast());
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn array_record_loading_and_untouched_output() {
        unsafe {
            assert_eq!(super::allocate(-1001, 2, 3, false), 0);
            let mut first = *b"12";
            assert_eq!(super::record(-1001, 1, &mut first, true), 0);
            let mut second = *b"34";
            assert_eq!(super::record(-1001, 2, &mut second, true), 0);
            for b in b"12\n34\n  \n" {
                assert_eq!(super::byte(-1001), Ok(Some(*b)));
            }
            assert_eq!(super::end(-1001, &mut [], false), 0);
            assert_eq!(super::allocate(-1001, 2, 3, true), 0);
            assert_eq!(super::write(-1001, b'7'), 0);
            let mut record = *b"xx";
            assert_eq!(super::record(-1001, 1, &mut record, false), 0);
            assert_eq!(&record, b"7 ");
            assert_eq!(super::record(-1001, 2, &mut record, false), 2);
            assert_eq!(super::end(-1001, &mut [], false), 0);
            assert_eq!(super::allocate(-1001, 2, 0, true), 0);
            assert_eq!(super::write(-1001, b'7'), -2);
            assert_eq!(super::end(-1001, &mut [], false), 0);
        }
    }

    #[test]
    fn records_bounds_and_cleanup() {
        unsafe {
            assert_eq!(super::begin(-1000, b"abcd", 2, 2, true), 0);
            for b in b"12\n34" {
                assert_eq!(super::write(-1000, *b), 0);
            }
            assert_eq!(super::write(-1000, b'5'), -2);
            let mut output = [0; 4];
            assert_eq!(super::end(-1000, &mut output, true), 0);
            assert_eq!(&output, b"1234");
            assert_eq!(super::mode(-1000), 0);
            assert_eq!(super::begin(-1000, b"abcd", 2, 2, false), 0);
            for b in b"ab\ncd\n" {
                assert_eq!(super::byte(-1000), Ok(Some(*b)));
            }
            assert_eq!(super::byte(-1000), Ok(None));
            assert_eq!(super::end(-1000, &mut [], false), 0);
            assert_eq!(super::mode(-1000), 0);
        }
    }
}

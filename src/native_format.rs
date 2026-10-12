// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Built once with the compiler and embedded as a native static library.
#![no_std]
mod format_spec;
mod record_input;
mod character_input;
use format_spec::GroupParser;

extern "C" {
    fn malloc(size:usize)->*mut core::ffi::c_void;
    fn realloc(pointer:*mut core::ffi::c_void,size:usize)->*mut core::ffi::c_void;
    fn free(pointer:*mut core::ffi::c_void);
    fn fgetc(stream: *mut core::ffi::c_void) -> i32;
    fn ferror(stream: *mut core::ffi::c_void) -> i32;
    fn tmpfile() -> *mut core::ffi::c_void;
    fn fwrite(data: *const u8, size: usize, count: usize, stream: *mut core::ffi::c_void) -> usize;
    fn fseek(stream:*mut core::ffi::c_void,offset:core::ffi::c_long,origin:i32)->i32;
    #[cfg(windows)] fn _fileno(stream:*mut core::ffi::c_void)->i32;
    #[cfg(windows)] fn _ftelli64(stream:*mut core::ffi::c_void)->i64;
    #[cfg(windows)] fn _chsize_s(fd:i32,length:i64)->i32;
    #[cfg(not(windows))] fn fileno(stream:*mut core::ffi::c_void)->i32;
    #[cfg(not(windows))] fn ftello(stream:*mut core::ffi::c_void)->i64;
    #[cfg(not(windows))] fn ftruncate(fd:i32,length:i64)->i32;
    #[cfg(windows)]
    fn __acrt_iob_func(index: u32) -> *mut core::ffi::c_void;
    #[cfg(target_os = "macos")]
    static mut __stdinp: *mut core::ffi::c_void;
    #[cfg(not(any(windows, target_os = "macos")))]
    static mut stdin: *mut core::ffi::c_void;
    fn printf(format: *const u8, ...) -> i32;
    fn sprintf(buffer: *mut u8, format: *const u8, ...) -> i32;
    fn putchar(c: i32) -> i32;
    fn exit(code: i32) -> !;
    fn fflush(stream: *mut core::ffi::c_void) -> i32;
    #[cfg(windows)]
    fn _write(fd: i32, data: *const u8, len: u32) -> i32;
    #[cfg(not(windows))]
    fn write(fd: i32, data: *const u8, len: usize) -> isize;
}

#[no_mangle]
pub unsafe extern "C" fn rfort_scratch_newunit(slots: *mut *mut core::ffi::c_void) -> i64 {
    for i in 1024..2048 {
        if (*slots.add(i)).is_null() {
            let file=tmpfile();
            if file.is_null() {return 0;}
            *slots.add(i)=file;
            return -10-(i-1024) as i64;
        }
    }
    0
}
#[no_mangle]
pub unsafe extern "C" fn rfort_file_write_record(stream:*mut core::ffi::c_void, data:*const u8, length:usize)->i32 {
    // Positioning permits switching from input to output on a C update stream.
    if fseek(stream,0,1)!=0 || fwrite(data,1,length,stream)!=length || fwrite(b"\n".as_ptr(),1,1,stream)!=1 || fflush(stream)!=0 {return 1;}
    // Sequential Fortran output replaces the remainder after the new record.
    #[cfg(windows)] {let pos=_ftelli64(stream);if pos<0 {1} else {_chsize_s(_fileno(stream),pos)}}
    #[cfg(not(windows))] {let pos=ftello(stream);if pos<0 {1} else {ftruncate(fileno(stream),pos)}}
}
#[no_mangle]
pub unsafe extern "C" fn rfort_file_name(source:*const u8, destination:*mut u8, length:usize)->i32 {
    let mut ended=source.is_null();
    for i in 0..length {
        let ch=if ended {b' '} else {*source.add(i)};
        if ch==0 {ended=true;}
        *destination.add(i)=if ended {b' '} else {ch};
    }
    *destination.add(length)=0;
    0
}

/// A null stream denotes standard input. The destination holds `length` bytes
/// plus a terminator; the compiler copies it to user storage only on success.
#[no_mangle]
pub unsafe extern "C" fn rfort_read_record(
    stream: *mut core::ffi::c_void, destination: *mut u8, length: usize,
) -> i64 {
    let stream = if stream.is_null() {
        #[cfg(windows)]
        { __acrt_iob_func(0) }
        #[cfg(target_os = "macos")]
        { __stdinp }
        #[cfg(not(any(windows, target_os = "macos")))]
        { stdin }
    } else { stream };
    let buffer = core::slice::from_raw_parts_mut(destination, length);
    let status = record_input::record(buffer, || {
        let value = fgetc(stream);
        if value >= 0 { Ok(Some(value as u8)) }
        else if ferror(stream) != 0 { Err(()) }
        else { Ok(None) }
    });
    *destination.add(length) = 0;
    status
}
#[no_mangle]
pub unsafe extern "C" fn rfort_read_record_format(
    stream: *mut core::ffi::c_void, destination: *mut u8, length: usize,
    format: *const u8, format_length: usize,
) -> i64 {
    if !record_input::valid_format(core::slice::from_raw_parts(format, format_length)) {
        return record_input::INVALID_FORMAT;
    }
    rfort_read_record(stream, destination, length)
}

#[no_mangle]
pub unsafe extern "C" fn rfort_read_character_field(source:*const u8,source_length:usize,destination:*mut u8,length:usize)->i64 {
    let code=character_input::field(core::slice::from_raw_parts(source,source_length),core::slice::from_raw_parts_mut(destination,length));
    *destination.add(length)=0;
    code
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
#[derive(Clone, Copy)]
pub struct Item {
    tag: u64,
    bits: u64,
    length: u64,
}
struct OutputList {format:*mut u8,length:usize,items:*mut Item,count:usize,capacity:usize,advance:i32}
unsafe fn allocate(size:usize)->*mut u8 {
    let pointer=malloc(size.max(1)) as *mut u8;
    if pointer.is_null() {fail("cannot allocate formatted output list");}
    pointer
}
#[no_mangle]
pub unsafe extern "C" fn rfort_format_begin(format:*const u8,length:usize,advance:i32)->*mut core::ffi::c_void {
    let copy=allocate(length);core::ptr::copy_nonoverlapping(format,copy,length);
    let items=allocate(core::mem::size_of::<Item>()) as *mut Item;
    let list=allocate(core::mem::size_of::<OutputList>()) as *mut OutputList;
    list.write(OutputList {format:copy,length,items,count:0,capacity:1,advance});
    list.cast()
}
#[no_mangle]
pub unsafe extern "C" fn rfort_format_push(list:*mut core::ffi::c_void,tag:u64,mut bits:u64,length:u64)->i32 {
    let list=&mut *list.cast::<OutputList>();
    if list.count==list.capacity {
        let capacity=list.capacity.checked_mul(2).unwrap_or_else(||fail("formatted output list is too large"));
        let size=capacity.checked_mul(core::mem::size_of::<Item>()).unwrap_or_else(||fail("formatted output list is too large"));
        let items=realloc(list.items.cast(),size) as *mut Item;
        if items.is_null() {fail("cannot allocate formatted output list");}
        list.items=items;list.capacity=capacity;
    }
    if tag==4 {
        let length=usize::try_from(length).unwrap_or_else(|_|fail("formatted output item is too large"));
        let copy=allocate(length.checked_add(1).unwrap_or_else(||fail("formatted output item is too large")));
        core::ptr::copy_nonoverlapping(bits as usize as *const u8,copy,length);
        *copy.add(length)=0;bits=copy as usize as u64;
    }
    list.items.add(list.count).write(Item {tag,bits,length});list.count+=1;
    0
}
#[no_mangle]
pub unsafe extern "C" fn rfort_format_end(pointer:*mut core::ffi::c_void)->i32 {
    let list=&*pointer.cast::<OutputList>();
    rfort_formatted_write(list.format,list.length,list.items,list.count,list.advance);
    for i in 0..list.count {
        let item=&*list.items.add(i);
        if item.tag==4 {free(item.bits as usize as *mut core::ffi::c_void);}
    }
    free(list.items.cast());free(list.format.cast());free(pointer);
    0
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
pub unsafe extern "C" fn rfort_formatted_write(
    format: *const u8,
    length: usize,
    items: *const Item,
    count: usize,
    advance: i32,
) {
    let bytes = core::slice::from_raw_parts(format, length);
    let items = core::slice::from_raw_parts(items, count);
    // Validate the entire statement before writing any output.
    let mut parser = checked(GroupParser::new(bytes,count));
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
    let mut parser = checked(GroupParser::new(bytes,count));
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
    if advance != 0 {
        putchar(10);
    } else {
        fflush(core::ptr::null_mut());
    }
}

// Preserve the advancing-only ABI used by previously emitted objects.
#[no_mangle]
pub unsafe extern "C" fn rfort_formatted_output(
    format: *const u8,
    length: usize,
    items: *const Item,
    count: usize,
) {
    rfort_formatted_write(format, length, items, count, 1);
}

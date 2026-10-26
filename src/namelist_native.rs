// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
use super::*;
struct Reader {
    session: namelist_input::Session,
    stream: *mut core::ffi::c_void,
    source: *mut u8,
    length: usize,
    offset: usize,
}
#[no_mangle]
pub unsafe extern "C" fn rfort_nml_begin(
    stream: *mut core::ffi::c_void,
    source: *const u8,
    length: usize,
    group: *const u8,
    group_length: usize,
) -> *mut core::ffi::c_void {
    let p = malloc(core::mem::size_of::<Reader>()).cast::<Reader>();
    if p.is_null() {
        return p.cast();
    }
    let copy = if source.is_null() {
        core::ptr::null_mut()
    } else {
        malloc(length.max(1)).cast::<u8>()
    };
    if !source.is_null() && copy.is_null() {
        free(p.cast());
        return core::ptr::null_mut();
    }
    if !source.is_null() {
        core::ptr::copy_nonoverlapping(source, copy, length);
    }
    p.write(Reader {
        session: namelist_input::Session::new(core::slice::from_raw_parts(group, group_length)),
        stream: input_stream(stream),
        source: copy,
        length,
        offset: 0,
    });
    p.cast()
}
#[no_mangle]
pub unsafe extern "C" fn rfort_nml_next(
    handle: *mut core::ffi::c_void,
    index: *mut i64,
    field: *mut u8,
    field_len: usize,
    token: *mut u8,
    token_len: usize,
) -> i64 {
    if handle.is_null() {
        return 1;
    }
    let r = &mut *handle.cast::<Reader>();
    let result = r.session.next(
        core::slice::from_raw_parts_mut(field, field_len),
        core::slice::from_raw_parts_mut(token, token_len),
        &mut *index,
        || reader_byte(r.stream, r.source, r.length, &mut r.offset),
    );
    *field.add(field_len) = 0;
    *token.add(token_len) = 0;
    result
}
#[no_mangle]
pub unsafe extern "C" fn rfort_nml_end(handle: *mut core::ffi::c_void) -> i64 {
    if !handle.is_null() {
        let p = handle.cast::<Reader>();
        free((*p).source.cast());
        core::ptr::drop_in_place(p);
        free(handle);
    }
    0
}
#[no_mangle]
pub unsafe extern "C" fn rfort_nml_record(
    slots: *const *mut core::ffi::c_void,
    unit: i64,
    data: *const u8,
    length: usize,
) -> i64 {
    let stream = if unit == 6 {
        #[cfg(windows)]
        {
            __acrt_iob_func(1)
        }
        #[cfg(target_os = "macos")]
        {
            extern "C" {
                static mut __stdoutp: *mut core::ffi::c_void;
            }
            __stdoutp
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            extern "C" {
                static mut stdout: *mut core::ffi::c_void;
            }
            stdout
        }
    } else {
        let Some(index) = file_inquiry::index(unit) else {
            return 1;
        };
        let f = *slots.add(index);
        if unit == 0 && f.is_null() {
            rfort_stderr_stream()
        } else {
            f
        }
    };
    if stream.is_null() || unit == 5 {
        return 1;
    }
    if unit == 6 || stream == rfort_stderr_stream() {
        i64::from(
            fwrite(data, 1, length, stream) != length
                || fputc(10, stream) < 0
                || fflush(stream) != 0,
        )
    } else {
        i64::from(rfort_file_write_record(stream, data, length) != 0)
    }
}

#[no_mangle]
pub unsafe extern "C" fn rfort_nml_layout(
    handle: *mut core::ffi::c_void,
    index: *mut i64,
    code: i64,
    axis: i64,
    rank: i64,
    lower: i64,
    upper: i64,
) -> i64 {
    if handle.is_null() {
        return 1;
    }
    (*handle.cast::<Reader>())
        .session
        .layout(&mut *index, code, axis, rank, lower, upper)
}

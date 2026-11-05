// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Unit-specific record buffers owned by the outermost defined output statement.
use crate::format_spec::OutputRecord;
use core::ptr;
extern "C" {
    fn malloc(n: usize) -> *mut core::ffi::c_void;
    fn free(p: *mut core::ffi::c_void);
}
struct Entry {
    unit: i64,
    depth: usize,
    record: OutputRecord,
    next: *mut Entry,
}
#[cfg(native_runtime)]
static mut HEAD: *mut Entry = ptr::null_mut();
#[cfg(not(native_runtime))]
std::thread_local! {static HEAD:core::cell::Cell<*mut Entry>=const{core::cell::Cell::new(ptr::null_mut())};}
unsafe fn head() -> *mut Entry {
    #[cfg(native_runtime)]
    {
        HEAD
    }
    #[cfg(not(native_runtime))]
    {
        HEAD.with(core::cell::Cell::get)
    }
}
unsafe fn set_head(p: *mut Entry) {
    #[cfg(native_runtime)]
    {
        HEAD = p;
    }
    #[cfg(not(native_runtime))]
    {
        HEAD.with(|h| h.set(p));
    }
}
unsafe fn find(unit: i64) -> *mut Entry {
    let mut p = head();
    while !p.is_null() {
        if (*p).unit == unit {
            return p;
        }
        p = (*p).next;
    }
    ptr::null_mut()
}
pub unsafe fn get(unit: i64) -> *mut OutputRecord {
    let p = find(unit);
    if p.is_null() {
        ptr::null_mut()
    } else {
        &mut (*p).record
    }
}
pub unsafe fn begin(unit: i64) -> bool {
    let p = find(unit);
    if !p.is_null() {
        (*p).depth += 1;
        return true;
    }
    let p = malloc(core::mem::size_of::<Entry>()).cast::<Entry>();
    if p.is_null() {
        return false;
    }
    let mut record = OutputRecord::new();
    record.limit = crate::internal_file::width(unit).unwrap_or(usize::MAX);
    p.write(Entry {
        unit,
        depth: 1,
        record,
        next: head(),
    });
    set_head(p);
    true
}
pub unsafe fn end(unit: i64, emit: &mut dyn FnMut(u8)) {
    let mut previous: *mut Entry = ptr::null_mut();
    let mut p = head();
    while !p.is_null() {
        if (*p).unit == unit {
            (*p).depth -= 1;
            if (*p).depth != 0 {
                return;
            }
            (*p).record.flush(emit);
            if previous.is_null() {
                set_head((*p).next);
            } else {
                (*previous).next = (*p).next;
            }
            ptr::drop_in_place(p);
            free(p.cast());
            return;
        }
        previous = p;
        p = (*p).next;
    }
}
#[cfg(not(native_runtime))]
pub unsafe fn clear() {
    while !head().is_null() {
        let p = head();
        set_head((*p).next);
        ptr::drop_in_place(p);
        free(p.cast());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ordinary_long_positioned_record() {
        let text = vec![b'a'; 5000];
        let mut output = vec![];
        crate::format_spec::render_with(
            b"(a,t1,a)",
            2,
            &|i, put| {
                put(crate::format_spec::Value::Character(if i == 0 {
                    &text
                } else {
                    b"X"
                }))
            },
            &mut |b| output.push(b),
            true,
        )
        .unwrap();
        assert_eq!(output.len(), 5000);
        assert_eq!(output[0], b'X');
        assert_eq!(output[4999], b'a');
    }

    #[test]
    fn nesting_and_cleanup() {
        unsafe {
            assert!(super::begin(91));
            assert!(super::begin(91));
            let state = &mut *super::get(91);
            let mut bytes = vec![];
            crate::format_spec::render_shared(
                b"(a)",
                1,
                &|_, put| put(crate::format_spec::Value::Character(b"abcd")),
                &mut |b| bytes.push(b),
                false,
                Some(state),
            )
            .unwrap();
            super::end(91, &mut |b| bytes.push(b));
            assert!(bytes.is_empty());
            crate::format_spec::render_shared(
                b"(t2,a)",
                1,
                &|_, put| put(crate::format_spec::Value::Character(b"X")),
                &mut |b| bytes.push(b),
                false,
                super::get(91).as_mut(),
            )
            .unwrap();
            super::end(91, &mut |b| bytes.push(b));
            assert_eq!(&bytes, b"aXcd");
            assert!(super::get(91).is_null());
        }
    }
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Numeric CFI descriptors and private conversion buffers. Also built no_std
//! into the embedded native runtime. Layout matches include/ISO_Fortran_binding.h.
use core::{ffi::c_void, ptr};
const MAX: usize = 15;
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
struct Dim {
    lower: isize,
    extent: isize,
    stride: isize,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Header {
    base: *mut u8,
    len: usize,
    version: i32,
    rank: i8,
    attribute: i8,
    typ: i16,
}
unsafe fn dims<'a>(d: *const Header) -> &'a [Dim] {
    core::slice::from_raw_parts(d.add(1).cast(), (*d).rank as usize)
}
fn width(typ: i16) -> Option<usize> {
    let w = (typ as u16 >> 8) as usize;
    match (typ & 255, w) {
        (1, 1 | 2 | 4 | 8) | (2, 4 | 8) => Some(w),
        _ => None,
    }
}
unsafe fn validate(d: *const Header, typ: i16, rank: i8) -> i32 {
    if d.is_null() {
        return 8;
    }
    let h = &*d;
    if h.version != 1 {
        return 8;
    }
    if h.rank < 0 || h.rank as usize > MAX || rank >= 0 && h.rank != rank {
        return 3;
    }
    if width(h.typ).is_none() || typ != 0 && h.typ != typ {
        return 4;
    }
    if Some(h.len) != width(h.typ) {
        return 2;
    }
    if h.attribute != 2 {
        return 5;
    }
    let ds = dims(d);
    if ds.iter().any(|a| a.extent < 0) {
        return 6;
    }
    let empty = ds.iter().any(|a| a.extent == 0);
    if !empty && h.base.is_null() {
        return 1;
    }
    if !empty && ds.iter().any(|a| a.extent > 1 && a.stride == 0) {
        return 7;
    }
    let Some((lo, hi, _)) = span(ds, h.len) else {
        return 6;
    };
    if !empty
        && ((h.base as usize).checked_add_signed(lo).is_none()
            || (h.base as usize)
                .checked_add_signed(hi)
                .and_then(|a| a.checked_add(h.len))
                .is_none())
    {
        return 6;
    }
    0
}
// Check arithmetic before any array access or allocation. Empty arrays do not
// access BASE_ADDR and may have zero strides following a zero extent.
fn span(ds: &[Dim], w: usize) -> Option<(isize, isize, usize)> {
    if ds.iter().any(|a| a.extent == 0) {
        return Some((0, 0, 0));
    }
    let (mut lo, mut hi, mut count) = (0isize, 0isize, 1usize);
    for a in ds {
        let n = usize::try_from(a.extent).ok()?;
        count = count.checked_mul(n)?;
        let step = (a.extent - 1).checked_mul(a.stride)?;
        lo = lo.checked_add(step.min(0))?;
        hi = hi.checked_add(step.max(0))?;
        a.lower.checked_add(a.extent - 1)?;
    }
    hi.checked_sub(lo)?.checked_add(w as isize)?;
    Some((lo, hi, count))
}
#[no_mangle]
pub unsafe extern "C" fn CFI_establish(
    d: *mut Header,
    base: *mut c_void,
    attribute: i8,
    typ: i16,
    elem_len: usize,
    rank: i8,
    extents: *const isize,
) -> i32 {
    if d.is_null() {
        return 8;
    }
    if rank < 0 || rank as usize > MAX {
        return 3;
    }
    if attribute != 2 {
        return 5;
    }
    let Some(w) = width(typ) else {
        return 4;
    };
    // For intrinsic numeric types ELEM_LEN is ignored by CFI_establish.
    let _ = elem_len;
    if rank > 0 && extents.is_null() {
        return 6;
    }
    let mut shape = [Dim::default(); MAX];
    let mut stride = w as isize;
    for i in 0..rank as usize {
        let n = *extents.add(i);
        if n < 0 {
            return 6;
        }
        shape[i] = Dim {
            lower: 0,
            extent: n,
            stride,
        };
        let Some(next) = stride.checked_mul(n) else {
            return 6;
        };
        stride = next;
    }
    *d = Header {
        base: base.cast(),
        len: w,
        version: 1,
        rank,
        attribute,
        typ,
    };
    ptr::copy_nonoverlapping(shape.as_ptr(), d.add(1).cast(), rank as usize);
    0
}
#[no_mangle]
pub unsafe extern "C" fn CFI_address(d: *const Header, subscripts: *const isize) -> *mut c_void {
    if validate(d, 0, -1) != 0 || (*d).rank > 0 && subscripts.is_null() {
        return ptr::null_mut();
    }
    let mut offset = 0isize;
    for (i, a) in dims(d).iter().enumerate() {
        let Some(j) = (*subscripts.add(i)).checked_sub(a.lower) else {
            return ptr::null_mut();
        };
        if j < 0 || j >= a.extent {
            return ptr::null_mut();
        }
        let Some(x) = j.checked_mul(a.stride).and_then(|x| offset.checked_add(x)) else {
            return ptr::null_mut();
        };
        offset = x;
    }
    (*d).base.wrapping_offset(offset).cast()
}
#[no_mangle]
pub unsafe extern "C" fn CFI_is_contiguous(d: *const Header) -> i32 {
    if validate(d, 0, -1) != 0 {
        return 0;
    }
    let ds = dims(d);
    if ds.iter().any(|a| a.extent == 0) {
        return 1;
    }
    let mut stride = (*d).len as isize;
    for a in ds {
        if a.extent > 1 && a.stride != stride {
            return 0;
        }
        let Some(next) = stride.checked_mul(a.extent) else {
            return 0;
        };
        stride = next;
    }
    1
}
extern "C" {
    fn malloc(n: usize) -> *mut c_void;
    fn free(p: *mut c_void);
    fn exit(n: i32) -> !;
    #[cfg(windows)]
    fn _write(fd: i32, p: *const u8, n: u32) -> i32;
    #[cfg(not(windows))]
    fn write(fd: i32, p: *const u8, n: usize) -> isize;
}
unsafe fn fail(code: i32) -> ! {
    let text: &[u8] = match code {
        1 => b"Runtime error: invalid C descriptor: null base address\n",
        2 => b"Runtime error: invalid C descriptor: element size\n",
        3 => b"Runtime error: invalid C descriptor: rank\n",
        4 => b"Runtime error: invalid C descriptor: type\n",
        5 => b"Runtime error: unsupported C descriptor attribute (requires CFI_attribute_other)\n",
        6 => b"Runtime error: invalid C descriptor: extent or address overflow\n",
        7 => b"Runtime error: invalid C descriptor: stride\n",
        9 => b"Runtime error: C descriptor allocation failed\n",
        11 => b"Runtime error: read of uninitialized C array argument element\n",
        12 => b"Runtime error: C procedure modified a nonallocatable array descriptor\n",
        _ => b"Runtime error: invalid C descriptor: version or descriptor pointer\n",
    };
    #[cfg(windows)]
    _write(2, text.as_ptr(), text.len() as u32);
    #[cfg(not(windows))]
    write(2, text.as_ptr(), text.len());
    exit(1)
}
#[no_mangle]
pub unsafe extern "C" fn rfort_c_validate(d: *const Header, typ: i64, rank: i64) -> i32 {
    let status = validate(d, typ as i16, rank as i8);
    if status != 0 {
        fail(status);
    }
    0
}
#[repr(C)]
struct Context {
    header: Header,
    dimensions: [Dim; MAX],
    original: Header,
    saved: [Dim; MAX],
    source: *mut u8,
    flags: *mut u8,
    source_strides: [isize; MAX],
    allocation: *mut c_void,
    raw: bool,
}
fn offset(mut ordinal: usize, ds: &[Dim], strides: impl Iterator<Item = isize>) -> isize {
    let mut result = 0;
    for (a, s) in ds.iter().zip(strides) {
        let digit = ordinal % a.extent as usize;
        ordinal /= a.extent as usize;
        result += digit as isize * s;
    }
    result
}
unsafe fn to_c(src: *const u8, dst: *mut u8, typ: i16) {
    match typ {
        257 => ptr::write_unaligned(
            dst.cast::<i8>(),
            ptr::read_unaligned(src.cast::<i64>()) as i8,
        ),
        513 => ptr::write_unaligned(
            dst.cast::<i16>(),
            ptr::read_unaligned(src.cast::<i64>()) as i16,
        ),
        1025 => ptr::write_unaligned(
            dst.cast::<i32>(),
            ptr::read_unaligned(src.cast::<i64>()) as i32,
        ),
        2049 => ptr::write_unaligned(dst.cast::<i64>(), ptr::read_unaligned(src.cast::<i64>())),
        1026 => ptr::write_unaligned(
            dst.cast::<f32>(),
            ptr::read_unaligned(src.cast::<f64>()) as f32,
        ),
        2050 => ptr::write_unaligned(dst.cast::<f64>(), ptr::read_unaligned(src.cast::<f64>())),
        _ => fail(4),
    }
}
unsafe fn from_c(src: *const u8, dst: *mut u8, typ: i16) {
    match typ {
        257 => ptr::write_unaligned(
            dst.cast::<i64>(),
            ptr::read_unaligned(src.cast::<i8>()) as i64,
        ),
        513 => ptr::write_unaligned(
            dst.cast::<i64>(),
            ptr::read_unaligned(src.cast::<i16>()) as i64,
        ),
        1025 => ptr::write_unaligned(
            dst.cast::<i64>(),
            ptr::read_unaligned(src.cast::<i32>()) as i64,
        ),
        2049 => ptr::write_unaligned(dst.cast::<i64>(), ptr::read_unaligned(src.cast::<i64>())),
        1026 => ptr::write_unaligned(
            dst.cast::<f64>(),
            ptr::read_unaligned(src.cast::<f32>()) as f64,
        ),
        2050 => ptr::write_unaligned(dst.cast::<f64>(), ptr::read_unaligned(src.cast::<f64>())),
        _ => fail(4),
    }
}
#[no_mangle]
pub unsafe extern "C" fn rfort_c_prepare(
    data: *mut u8,
    flags: *mut u8,
    rank: i64,
    typ: i64,
    raw: i64,
    input: i64,
    shape: *const isize,
) -> *mut Header {
    if !(0..=MAX as i64).contains(&rank) {
        fail(3);
    }
    let Some(w) = width(typ as i16) else {
        fail(4);
    };
    let mut ds = [Dim::default(); MAX];
    let mut source_strides = [0isize; MAX];
    for i in 0..rank as usize {
        let extent = *shape.add(2 * i);
        let stride = *shape.add(2 * i + 1);
        if extent < 0 || raw == 0 && stride % 8 != 0 {
            fail(6);
        }
        source_strides[i] = stride;
        let sm = if raw != 0 {
            stride
        } else {
            let Some(s) = (stride / 8).checked_mul(w as isize) else {
                fail(6);
            };
            s
        };
        ds[i] = Dim {
            lower: 0,
            extent,
            stride: sm,
        };
    }
    let Some((lo, hi, count)) = span(&ds[..rank as usize], w) else {
        fail(6);
    };
    let mut source_dims = ds;
    for i in 0..rank as usize {
        source_dims[i].stride = source_strides[i];
    }
    if span(&source_dims[..rank as usize], 8).is_none() {
        fail(6);
    }
    let allocation = if raw != 0 {
        ptr::null_mut()
    } else {
        let bytes = if count == 0 {
            1
        } else {
            (hi - lo) as usize + w
        };
        let p = malloc(bytes);
        if p.is_null() {
            fail(9);
        }
        p
    };
    let base = if raw != 0 {
        data
    } else {
        allocation.cast::<u8>().wrapping_offset(-lo)
    };
    let h = Header {
        base,
        len: w,
        version: 1,
        rank: rank as i8,
        attribute: 2,
        typ: typ as i16,
    };
    let context = malloc(core::mem::size_of::<Context>()).cast::<Context>();
    if context.is_null() {
        free(allocation);
        fail(9);
    }
    ptr::write(
        context,
        Context {
            header: h,
            dimensions: ds,
            original: h,
            saved: ds,
            source: data,
            flags,
            source_strides,
            allocation,
            raw: raw != 0,
        },
    );
    let status = validate(context.cast(), typ as i16, rank as i8);
    if status != 0 {
        fail(status);
    }
    if raw == 0 && input != 0 {
        for j in 0..count {
            let src = offset(j, &ds[..rank as usize], source_strides.into_iter());
            if ptr::read_unaligned(flags.wrapping_offset(src).cast::<i64>()) == 0 {
                fail(11);
            }
            let dst = offset(j, &ds[..rank as usize], ds.iter().map(|a| a.stride));
            to_c(
                data.wrapping_offset(src),
                base.wrapping_offset(dst),
                typ as i16,
            );
        }
    }
    context.cast()
}
#[no_mangle]
pub unsafe extern "C" fn rfort_c_release(d: *mut Header, output: i64) -> i32 {
    let c = &*d.cast::<Context>();
    let n = c.original.rank as usize;
    let h = &c.header;
    let o = &c.original;
    if h.base != o.base
        || h.len != o.len
        || h.version != o.version
        || h.rank != o.rank
        || h.attribute != o.attribute
        || h.typ != o.typ
        || c.dimensions[..n] != c.saved[..n]
    {
        fail(12);
    }
    if !c.raw && output != 0 {
        let (_, _, count) = span(&c.saved[..n], h.len).unwrap();
        for j in 0..count {
            let src = offset(j, &c.saved[..n], c.saved.iter().map(|a| a.stride));
            let dst = offset(j, &c.saved[..n], c.source_strides.into_iter());
            from_c(
                h.base.wrapping_offset(src),
                c.source.wrapping_offset(dst),
                h.typ,
            );
            ptr::write_unaligned(c.flags.wrapping_offset(dst).cast::<i64>(), 1);
        }
    }
    free(c.allocation);
    free(d.cast());
    0
}

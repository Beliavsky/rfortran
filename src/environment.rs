// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.
//! Environment lookup shared by interpreted and native execution.
extern "C" {
    fn malloc(size:usize)->*mut core::ffi::c_void;
    fn free(pointer:*mut core::ffi::c_void);
    fn getenv(name:*const u8)->*const u8;
}
/// Output has width+1 writable bytes; the extra byte holds the C terminator.
#[no_mangle]
pub unsafe extern "C" fn rfort_environment_query(name:*const u8,name_len:usize,trim:i64,output:*mut u8,width:usize,present:i64,length:*mut i64)->i64 {
    *length=0;
    let mut size=name_len;
    if trim!=0 {while size>0 && *name.add(size-1)==b' ' {size-=1;}}
    // An embedded NUL is not a host environment-variable name.
    let mut status=1;
    let mut found=core::ptr::null();
    let key=malloc(size.saturating_add(1)) as *mut u8;
    if key.is_null() {status=2;}
    else {
        core::ptr::copy_nonoverlapping(name,key,size);*key.add(size)=0;
        if !core::slice::from_raw_parts(key,size).contains(&0) {found=getenv(key);}
        free(key.cast());
    }
    if !found.is_null() {
        let mut n=0;while *found.add(n)!=0 {n+=1;}
        *length=n as i64;
        status=if present!=0 && n>width {-1} else {0};
        let copied=n.min(width);core::ptr::copy_nonoverlapping(found,output,copied);
        core::ptr::write_bytes(output.add(copied),b' ',width-copied);
    } else {core::ptr::write_bytes(output,b' ',width);}
    *output.add(width)=0;
    status
}

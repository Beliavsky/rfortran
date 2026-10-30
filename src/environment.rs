// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.
//! Host environment and current-directory inquiries shared by both execution paths.
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

/// GNU GETCWD subroutine: blank output on failure, including an undersized buffer.
#[no_mangle]
pub unsafe extern "C" fn rfort_getcwd(output:*mut u8,width:usize)->i64 {
    extern "C" {
        #[cfg(windows)] fn _getcwd(buffer:*mut u8,size:i32)->*mut u8;
        #[cfg(not(windows))] fn getcwd(buffer:*mut u8,size:usize)->*mut u8;
        #[cfg(windows)] fn _errno()->*mut i32;
        #[cfg(target_os="macos")] fn __error()->*mut i32;
        #[cfg(not(any(windows,target_os="macos")))] fn __errno_location()->*mut i32;
    }
    core::ptr::write_bytes(output,b' ',width);*output.add(width)=0;
    if width==0 {return 22;}
    let mut capacity=256usize;
    loop {
        #[cfg(windows)] if capacity>i32::MAX as usize {return 34;}
        let buffer=malloc(capacity) as *mut u8;
        if buffer.is_null() {return 12;}
        #[cfg(windows)] let result=_getcwd(buffer,capacity as i32);
        #[cfg(not(windows))] let result=getcwd(buffer,capacity);
        if !result.is_null() {
            let mut length=0;while *buffer.add(length)!=0 {length+=1;}
            let status=if length>width {34} else {core::ptr::copy_nonoverlapping(buffer,output,length);0};
            free(buffer.cast());return status;
        }
        #[cfg(windows)] let code=*_errno();
        #[cfg(target_os="macos")] let code=*__error();
        #[cfg(not(any(windows,target_os="macos")))] let code=*__errno_location();
        free(buffer.cast());
        if code!=34 {return i64::from(if code==0 {1} else {code});}
        let Some(next)=capacity.checked_mul(2) else {return 12;};capacity=next;
    }
}

// The native main wrapper sets these once, before any Fortran code executes.
static mut ARG_COUNT:i32=0;
static mut ARG_VALUES:*const *const u8=core::ptr::null();
#[no_mangle]
pub unsafe extern "C" fn rfort_set_arguments(count:i32,values:*const *const u8)->i64 {
    ARG_COUNT=count;ARG_VALUES=values;
    // Called once at the start of every compiled main program.
    #[cfg(native_runtime)]
    crate::watch_standard_output();
    0
}
#[no_mangle]
pub unsafe extern "C" fn rfort_argument_query(number:i64,output:*mut u8,width:usize,present:i64,length:*mut i64)->i64 {
    *length=0;core::ptr::write_bytes(output,b' ',width);*output.add(width)=0;
    if number<0 || number>=i64::from(ARG_COUNT) || ARG_VALUES.is_null() {return 1;}
    let value=*ARG_VALUES.add(number as usize);
    if value.is_null() {return 1;}
    let mut size=0;while *value.add(size)!=0 {size+=1;}
    *length=size as i64;
    core::ptr::copy_nonoverlapping(value,output,size.min(width));
    if present!=0 && size>width {-1} else {0}
}

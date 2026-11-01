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
/// The argument NUMBER that asks rfort_argument_query for the whole command,
/// as GET_COMMAND returns it.
pub const COMMAND_LINE:i64=-2147483647;
/// COMMAND_ARGUMENT_COUNT: the arguments after the command name.
#[no_mangle]
pub unsafe extern "C" fn rfort_argument_count()->i64 {
    i64::from(ARG_COUNT.max(1)-1)
}
#[no_mangle]
pub unsafe extern "C" fn rfort_argument_query(number:i64,output:*mut u8,width:usize,present:i64,length:*mut i64)->i64 {
    *length=0;core::ptr::write_bytes(output,b' ',width);*output.add(width)=0;
    if number==COMMAND_LINE {
        // The command name and arguments, separated by blanks.
        if ARG_VALUES.is_null() {return 1;}
        let mut size=0usize;
        for k in 0..ARG_COUNT.max(0) as usize {
            let value=*ARG_VALUES.add(k);
            if value.is_null() {continue;}
            if k>0 {if size<width {*output.add(size)=b' ';} size+=1;}
            let mut n=0;
            while *value.add(n)!=0 {if size<width {*output.add(size)=*value.add(n);} size+=1;n+=1;}
        }
        *length=size as i64;
        return if present!=0 && size>width {-1} else {0};
    }
    if number<0 || number>=i64::from(ARG_COUNT) || ARG_VALUES.is_null() {return 1;}
    let value=*ARG_VALUES.add(number as usize);
    if value.is_null() {return 1;}
    let mut size=0;while *value.add(size)!=0 {size+=1;}
    *length=size as i64;
    core::ptr::copy_nonoverlapping(value,output,size.min(width));
    if present!=0 && size>width {-1} else {0}
}

/// EXECUTE_COMMAND_LINE, run synchronously: the CMDSTAT value, 0 when the
/// command ran (or -2 when WAIT is false), with its exit status in EXITSTAT.
/// MESSAGE, of WIDTH bytes and a terminator, describes a failure.
#[no_mangle]
pub unsafe extern "C" fn rfort_execute_command(command:*const u8,length:usize,wait:i64,message:*mut u8,width:usize,exitstat:*mut i64)->i64 {
    core::ptr::write_bytes(message,b' ',width);*message.add(width)=0;
    let (status,text):(i64,&[u8])=run_command(command,length,exitstat);
    let copied=text.len().min(width);core::ptr::copy_nonoverlapping(text.as_ptr(),message,copied);
    if status==0 && wait==0 {-2} else {status}
}
#[cfg(target_os="wasi")]
unsafe fn run_command(_command:*const u8,_length:usize,_exitstat:*mut i64)->(i64,&'static [u8]) {
    (-1,b"command execution is not supported")
}
#[cfg(not(target_os="wasi"))]
unsafe fn run_command(command:*const u8,length:usize,exitstat:*mut i64)->(i64,&'static [u8]) {
    extern "C" {fn system(command:*const u8)->i32;fn fflush(stream:*mut core::ffi::c_void)->i32;}
    let buffer=malloc(length.saturating_add(1)) as *mut u8;
    if buffer.is_null() {return (2,b"out of memory");}
    core::ptr::copy_nonoverlapping(command,buffer,length);*buffer.add(length)=0;
    if core::slice::from_raw_parts(buffer,length).contains(&0) {free(buffer.cast());return (3,b"the command contains a NUL character");}
    // Output written so far comes before the command's own output.
    fflush(core::ptr::null_mut());
    let code=system(buffer);
    free(buffer.cast());
    if code==-1 {return (1,b"the command could not be executed");}
    #[cfg(windows)] {*exitstat=i64::from(code);}
    // A wait status: the exit code, or 128 plus the signal that ended it.
    #[cfg(not(windows))] {*exitstat=i64::from(if code&0x7f==0 {(code>>8)&0xff} else {128+(code&0x7f)});}
    (0,b"")
}

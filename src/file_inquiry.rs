// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Connection-query helpers shared by the interpreter and native runtime.
use core::ffi::c_void;

pub(crate) fn invalid_unit(unit:i64)->bool {matches!(unit,-1|-2)}
pub(crate) fn index(unit:i64)->Option<usize> {
    if (0..1024).contains(&unit) {Some(unit as usize)}
    else if (-1033..=-10).contains(&unit) {Some((1024-unit-10) as usize)}
    else {None}
}
pub(crate) fn unit(index:usize)->i64 {if index<1024 {index as i64} else {-10-(index-1024) as i64}}

/// ACTION codes stored with a connection, independently of the C stream mode.
pub(crate) fn action(mode:i64)->i64 {match mode {0=>1,1|4|6=>2,_=>3}}
pub(crate) fn action_text(action:i64)->&'static str {
    match action {1=>"READ",2=>"WRITE",3=>"READWRITE",_=>"UNDEFINED"}
}

/// Test filesystem existence without opening, truncating, or connecting a file.
/// A missing path is a successful inquiry; other OS failures are I/O errors.
pub(crate) fn exists(path:&[u8])->(i64,i64) {
    let path=trim(path);
    if path.contains(&0) {return (2,0);}
    if path.is_empty() {return (0,0);}
    unsafe {
        let name=malloc(path.len()+1).cast::<u8>();
        if name.is_null() {return (3,0);}
        core::ptr::copy_nonoverlapping(path.as_ptr(),name,path.len());
        *name.add(path.len())=0;
        #[cfg(windows)] let result=_access(name,0);
        #[cfg(not(windows))] let result=access(name,0);
        #[cfg(windows)] let error=*_errno();
        #[cfg(target_os="macos")] let error=*__error();
        #[cfg(not(any(windows,target_os="macos")))] let error=*__errno_location();
        free(name.cast());
        if result==0 {(0,1)} else if matches!(error,2|20) {(0,0)} else {(3,0)}
    }
}

extern "C" {
    #[cfg(windows)] fn _access(path:*const u8,mode:i32)->i32;
    #[cfg(not(windows))] fn access(path:*const u8,mode:i32)->i32;
    #[cfg(windows)] fn _errno()->*mut i32;
    #[cfg(target_os="macos")] fn __error()->*mut i32;
    #[cfg(not(any(windows,target_os="macos")))] fn __errno_location()->*mut i32;
    fn malloc(length:usize)->*mut c_void;
    fn free(pointer:*mut c_void);
    #[cfg(windows)] fn _fullpath(destination:*mut u8,path:*const u8,length:usize)->*mut u8;
    #[cfg(not(windows))] fn realpath(path:*const u8,destination:*mut u8)->*mut u8;
}
struct Path(*mut u8);
impl Drop for Path {fn drop(&mut self){unsafe{free(self.0.cast())}}}
impl Path {
    fn canonical(path:&[u8])->Option<Self> {
        if path.contains(&0) {return None;}
        unsafe {
            let input=malloc(path.len().checked_add(1)?) as *mut u8;
            if input.is_null() {return None;}
            core::ptr::copy_nonoverlapping(path.as_ptr(),input,path.len());*input.add(path.len())=0;
            #[cfg(windows)] let result=_fullpath(core::ptr::null_mut(),input,0);
            #[cfg(not(windows))] let result=realpath(input,core::ptr::null_mut());
            free(input.cast());
            if result.is_null() {None} else {Some(Self(result))}
        }
    }
}
pub(crate) fn trim(mut path:&[u8])->&[u8] {while path.last()==Some(&b' ') {path=&path[..path.len()-1];}path}
/// Compare normalized absolute paths; this does not equate distinct hard links.
pub(crate) fn same_name(a:&[u8],b:&[u8])->bool {
    let (a,b)=(trim(a),trim(b));
    if a==b {return true;}
    let (Some(a),Some(b))=(Path::canonical(a),Path::canonical(b)) else {return false;};
    unsafe {
        let mut i=0;
        loop {
            let (x,y)=(*a.0.add(i),*b.0.add(i));
            #[cfg(windows)] let (x,y)=(x.to_ascii_lowercase(),y.to_ascii_lowercase());
            if x!=y {return false;}
            if x==0 {return true;}i+=1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existence_and_invalid_paths() {
        assert_eq!(exists(b""),(0,0));
        assert_eq!(exists(b"   "),(0,0));
        assert_eq!(exists(b"a\0b"),(2,0));
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let source=root.join("src/file_inquiry.rs").to_string_lossy().into_owned();
        assert_eq!(exists(source.as_bytes()),(0,1));
        assert_eq!(exists(format!("{source}   ").as_bytes()),(0,1));
        let missing=root.join("target/inquire-missing-parent/no-such-file").to_string_lossy().into_owned();
        assert_eq!(exists(missing.as_bytes()),(0,0));
    }

    #[test]
    fn connection_actions() {
        for (mode,expected) in [(0,"READ"),(1,"WRITE"),(2,"READWRITE"),(3,"READWRITE"),(4,"WRITE"),(5,"READWRITE"),(6,"WRITE")] {
            assert_eq!(action_text(action(mode)),expected);
        }
        assert_eq!(action_text(0),"UNDEFINED");
    }
}

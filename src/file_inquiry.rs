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

extern "C" {
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

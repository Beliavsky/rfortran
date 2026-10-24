// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Built once with the compiler and embedded as a native static library.
#![no_std]
mod timing;
mod float_model;

#[no_mangle]
pub extern "C" fn rfort_float_model(op:i64,kind:i64,x:f64,i:i64,s:f64)->f64 {
    if op==4 && s==0.0 {
        let message=b"Runtime error: NEAREST S must not be zero\n";
        unsafe {
            #[cfg(windows)] _write(2,message.as_ptr(),message.len() as u32);
            #[cfg(not(windows))] write(2,message.as_ptr(),message.len());
            exit(1);
        }
    }
    float_model::evaluate(op,kind as u8,x,i,s)
}
/// BESSEL_JN(N, X) and BESSEL_YN(N, X): imported math routines take REAL
/// arguments, so X comes first and the order N arrives as a whole REAL.
#[no_mangle]
pub extern "C" fn rfort_bessel_jn(x:f64,n:f64)->f64 {
    extern "C" {#[cfg_attr(windows,link_name="_jn")] fn jn(n:i32,x:f64)->f64;}
    unsafe {jn(n as i32,x)}
}
#[no_mangle]
pub extern "C" fn rfort_bessel_yn(x:f64,n:f64)->f64 {
    extern "C" {#[cfg_attr(windows,link_name="_yn")] fn yn(n:i32,x:f64)->f64;}
    unsafe {yn(n as i32,x)}
}
mod calendar;
mod environment;
mod format_spec;
mod record_input;
mod list_input;
mod character_input;
mod file_inquiry;
use format_spec::GroupParser;

extern "C" {
    fn malloc(size:usize)->*mut core::ffi::c_void;
    fn realloc(pointer:*mut core::ffi::c_void,size:usize)->*mut core::ffi::c_void;
    fn free(pointer:*mut core::ffi::c_void);
    fn fopen(path:*const u8,mode:*const u8)->*mut core::ffi::c_void;
    fn fputc(c:i32,stream:*mut core::ffi::c_void)->i32;
    fn fgetc(stream: *mut core::ffi::c_void) -> i32;
    fn ferror(stream: *mut core::ffi::c_void) -> i32;
    fn tmpfile() -> *mut core::ffi::c_void;
    fn fwrite(data: *const u8, size: usize, count: usize, stream: *mut core::ffi::c_void) -> usize;
    fn fread(data: *mut u8, size: usize, count: usize, stream: *mut core::ffi::c_void) -> usize;
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
/// Standard error, the stream of unit 0 when no file is connected to it.
#[no_mangle]
pub unsafe extern "C" fn rfort_stderr_stream()->*mut core::ffi::c_void {
    #[cfg(windows)] {__acrt_iob_func(2)}
    #[cfg(target_os="macos")] {extern "C" {static mut __stderrp:*mut core::ffi::c_void;} __stderrp}
    #[cfg(not(any(windows,target_os="macos")))] {extern "C" {static mut stderr:*mut core::ffi::c_void;} stderr}
}
#[no_mangle]
pub unsafe extern "C" fn rfort_file_write_record(stream:*mut core::ffi::c_void, data:*const u8, length:usize)->i32 {
    if stream==rfort_stderr_stream() {
        return i32::from(fwrite(data,1,length,stream)!=length || fwrite(b"
".as_ptr(),1,1,stream)!=1 || fflush(stream)!=0);
    }
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

#[no_mangle]
pub unsafe extern "C" fn rfort_file_inquire(
    slots:*const *mut core::ffi::c_void, names:*const *const u8, actions:*const i64,unit:i64,
    path:*const u8,path_length:usize,number:*mut i64,opened:*mut i64,exists:*mut i64,named:*mut i64,
    destination:*mut u8,length:usize,action_destination:*mut u8,action_length:usize,
)->i64 {
    *number=-1;*opened=0;*exists=0;*named=0;
    let mut source=core::ptr::null();let mut status=0;let mut action=0;
    if !path.is_null() {
        let path_slice=file_inquiry::trim(core::slice::from_raw_parts(path,path_length));
        (status,*exists)=file_inquiry::exists(path_slice);
        if status==0 {
            *named=1;
            for i in 0..2048 {
                let name=*names.add(i);
                if !(*slots.add(i)).is_null() && !name.is_null() {
                    let mut n=0;while *name.add(n)!=0 {n+=1;}
                    if file_inquiry::same_name(path_slice,core::slice::from_raw_parts(name,n)) {
                        *number=file_inquiry::unit(i);*opened=1;action=*actions.add(i);break;
                    }
                }
            }
            // FILE inquiry returns the supplied name even if not connected.
            let copy=length.min(path_slice.len());
            core::ptr::copy_nonoverlapping(path, destination,copy);
            core::ptr::write_bytes(destination.add(copy),b' ',length-copy);
            *destination.add(length)=0;
        } else {rfort_file_name(source,destination,length);}
    } else {
        if file_inquiry::invalid_unit(unit) {status=1;}
        else {
            let index=file_inquiry::index(unit);
            *exists=i64::from(unit>=0 && index.is_some());
            if let Some(index)=index.filter(|i|!(*slots.add(*i)).is_null()) {
                *number=unit;*opened=1;*exists=1;source=*names.add(index);*named=i64::from(!source.is_null());action=*actions.add(index);
            } else if matches!(unit,0|5|6) {
                *number=unit;*opened=1;*named=1;
                source=match unit {0=>b"stderr\0".as_ptr(),5=>b"stdin\0".as_ptr(),_=>b"stdout\0".as_ptr()};
                action=if unit==5 {1}else{2};
            }
        }
        rfort_file_name(source,destination,length);
    }
    let text=file_inquiry::action_text(action).as_bytes();
    let copy=action_length.min(text.len());
    core::ptr::copy_nonoverlapping(text.as_ptr(),action_destination,copy);
    core::ptr::write_bytes(action_destination.add(copy),b' ',action_length-copy);
    *action_destination.add(action_length)=0;
    status
}

#[no_mangle]
pub unsafe extern "C" fn rfort_file_free_name(path:*mut core::ffi::c_void)->i32 {free(path);0}
/// Delete the named file of a closed unit; a scratch unit has no name.
#[no_mangle]
pub unsafe extern "C" fn rfort_file_delete(path:*const u8)->i32 {
    extern "C" {fn remove(path:*const u8)->i32;}
    if path.is_null() {0} else {i32::from(remove(path)!=0)}
}
/// List-directed REAL text, NUL-terminated in a buffer of at least 64 bytes.
#[no_mangle]
pub unsafe extern "C" fn rfort_real_text(x:f64,digits:i32,buffer:*mut u8)->*mut u8 {
    let out=core::slice::from_raw_parts_mut(buffer,63);
    let n=format_spec::list_real(x,digits.max(1) as usize,out);
    *buffer.add(n)=0;
    buffer
}
/// ENDFILE: end the file at the current position.
#[no_mangle]
pub unsafe extern "C" fn rfort_file_endfile(stream:*mut core::ffi::c_void)->i32 {
    if fflush(stream)!=0 {return 1;}
    #[cfg(windows)] {let pos=_ftelli64(stream);if pos<0 || _chsize_s(_fileno(stream),pos)!=0 {return 1;}}
    #[cfg(not(windows))] {let pos=ftello(stream);if pos<0 || ftruncate(fileno(stream),pos)!=0 {return 1;}}
    0
}
#[no_mangle]
pub unsafe extern "C" fn rfort_file_open_dynamic(slots:*mut *mut core::ffi::c_void,names:*mut *const u8,actions:*mut i64,requested:i64,path:*const u8,length:usize,mode:i64,newunit:i64)->i64 {
    let mut length=length;while length>0 && *path.add(length-1)==b' ' {length-=1;}
    if length==0 || core::slice::from_raw_parts(path,length).contains(&0) {return i64::MIN;}
    let unit=if newunit!=0 {
        let mut unit=0;for i in 1024..2048 {if (*slots.add(i)).is_null() {unit=file_inquiry::unit(i);break;}}unit
    }else{requested};
    if newunit!=0 && unit==0 {return i64::MIN;}
    if matches!(unit,5|6) {return i64::MIN+2;}let Some(index)=file_inquiry::index(unit) else {return i64::MIN+2;};
    if !(*slots.add(index)).is_null() {return i64::MIN+1;}
    let name=malloc(length+1).cast::<u8>();if name.is_null() {return i64::MIN;}
    core::ptr::copy_nonoverlapping(path,name,length);*name.add(length)=0;
    let flags=match mode {0=>b"rb\0".as_ptr(),1=>b"wb\0".as_ptr(),2=>b"w+b\0".as_ptr(),3|4|5|6=>b"r+b\0".as_ptr(),_=>{free(name.cast());return i64::MIN;}};
    let mut file=fopen(name,flags);if file.is_null() && matches!(mode,5|6) {file=fopen(name,b"w+b\0".as_ptr());}
    if file.is_null() {free(name.cast());return i64::MIN;}
    *slots.add(index)=file;*names.add(index)=name;*actions.add(index)=file_inquiry::action(mode);unit
}
#[no_mangle]
pub unsafe extern "C" fn rfort_format_end_file(pointer:*mut core::ffi::c_void,stream:*mut core::ffi::c_void)->i32 {
    let list=&*pointer.cast::<OutputList>();let mut failed=false;
    if stream==rfort_stderr_stream() {
        render_format(list.format,list.length,list.items,list.count,&mut |c|{if fputc(c as i32,stream)<0 {failed=true;}},list.advance!=0);
        if list.advance!=0 && fputc(10,stream)<0 {failed=true;}
        if fflush(stream)!=0 {failed=true;}
        free_output_list(pointer);
        return failed as i32;
    }
    if fseek(stream,0,1)!=0 {failed=true;}
    render_format(list.format,list.length,list.items,list.count,&mut |c|{if fputc(c as i32,stream)<0 {failed=true;}},list.advance!=0);
    if list.advance!=0 && fputc(10,stream)<0 {failed=true;}
    if fflush(stream)!=0 {failed=true;}
    #[cfg(windows)] {let pos=_ftelli64(stream);if pos<0 || _chsize_s(_fileno(stream),pos)!=0 {failed=true;}}
    #[cfg(not(windows))] {let pos=ftello(stream);if pos<0 || ftruncate(fileno(stream),pos)!=0 {failed=true;}}
    free_output_list(pointer);failed as i32
}

unsafe fn input_stream(stream:*mut core::ffi::c_void)->*mut core::ffi::c_void {
    if !stream.is_null() {return stream;}
    #[cfg(windows)] {__acrt_iob_func(0)}
    #[cfg(target_os="macos")] {__stdinp}
    #[cfg(not(any(windows,target_os="macos")))] {stdin}
}
#[repr(C)]
struct ListReader {session:list_input::Session,stream:*mut core::ffi::c_void,source:*mut u8,length:usize,offset:usize}
#[no_mangle]
pub unsafe extern "C" fn rfort_list_read_begin(stream:*mut core::ffi::c_void)->*mut core::ffi::c_void {
    let p=malloc(core::mem::size_of::<ListReader>()).cast::<ListReader>();
    if !p.is_null() {p.write(ListReader {session:list_input::Session::new(),stream:input_stream(stream),source:core::ptr::null_mut(),length:0,offset:0});} p.cast()
}
#[no_mangle]
pub unsafe extern "C" fn rfort_list_read_internal_begin(source:*const u8,length:usize)->*mut core::ffi::c_void {
    let p=malloc(core::mem::size_of::<ListReader>()).cast::<ListReader>();if p.is_null() {return p.cast();}
    let copy=malloc(length.max(1)).cast::<u8>();if copy.is_null() {free(p.cast());return core::ptr::null_mut();}
    core::ptr::copy_nonoverlapping(source,copy,length);
    p.write(ListReader{session:list_input::Session::new(),stream:core::ptr::null_mut(),source:copy,length,offset:0});p.cast()
}
unsafe fn reader_byte(stream:*mut core::ffi::c_void,source:*mut u8,length:usize,offset:&mut usize)->Result<Option<u8>,()> {
    if source.is_null() {input_byte(stream)}else if *offset==length {Ok(None)}else{let c=*source.add(*offset);*offset+=1;Ok(Some(c))}
}
unsafe fn input_byte(stream:*mut core::ffi::c_void)->Result<Option<u8>,()> {
    let c=fgetc(stream);if c>=0 {Ok(Some(c as u8))}else if ferror(stream)!=0 {Err(())}else{Ok(None)}
}
/// Unformatted stream output of one scalar, as the bytes of its kind, from
/// compiled storage: INTEGER and REAL slots hold 8 bytes, LOGICAL one byte
/// and CHARACTER `width` bytes. Returns 0, or 1 on an error.
#[no_mangle]
pub unsafe extern "C" fn rfort_stream_put(stream:*mut core::ffi::c_void,tag:i64,width:usize,slot:*const u8)->i64 {
    let mut bytes=[0u8;8];
    let data:&[u8]=match tag {
        1=>{bytes=core::ptr::read_unaligned(slot.cast::<i64>()).to_le_bytes();&bytes[..width]}
        2=>{let value=core::ptr::read_unaligned(slot.cast::<f64>());
            if width==4 {bytes[..4].copy_from_slice(&(value as f32).to_le_bytes());&bytes[..4]} else {bytes=value.to_le_bytes();&bytes[..]}}
        3=>{bytes[..4].copy_from_slice(&i32::from(*slot!=0).to_le_bytes());&bytes[..4]}
        _=>core::slice::from_raw_parts(slot,width),
    };
    i64::from(fwrite(data.as_ptr(),1,data.len(),stream)!=data.len())
}
/// Unformatted stream input of one scalar into compiled storage. Returns 0,
/// -1 at end of file, or 1 on an error.
#[no_mangle]
pub unsafe extern "C" fn rfort_stream_get(stream:*mut core::ffi::c_void,tag:i64,width:usize,slot:*mut u8)->i64 {
    let mut bytes=[0u8;8];
    let buffer:&mut [u8]=if tag==4 {core::slice::from_raw_parts_mut(slot,width)} else {&mut bytes[..width]};
    let count=fread(buffer.as_mut_ptr(),1,width,stream);
    if count!=width {return if ferror(stream)!=0 {1} else {-1};}
    match tag {
        // Sign-extend from the value's kind.
        1=>{if bytes[width-1]&0x80!=0 {bytes[width..].fill(0xff);}core::ptr::write_unaligned(slot.cast::<i64>(),i64::from_le_bytes(bytes));}
        2=>{let value=if width==4 {f64::from(f32::from_le_bytes(bytes[..4].try_into().unwrap()))} else {f64::from_le_bytes(bytes)};
            core::ptr::write_unaligned(slot.cast::<f64>(),value);}
        3=>{*slot=u8::from(bytes[..width].iter().any(|b|*b!=0));}
        _=>{}
    }
    0
}
/// Position a stream file at byte POS, counted from 1.
#[no_mangle]
pub unsafe extern "C" fn rfort_stream_seek(stream:*mut core::ffi::c_void,position:i64)->i64 {
    if position<1 {return 1;}
    i64::from(fseek(stream,(position-1) as core::ffi::c_long,0)!=0)
}
#[no_mangle]
pub unsafe extern "C" fn rfort_list_read_next(handle:*mut core::ffi::c_void,kind:i64,width:usize,out:*mut u8)->i64 {
    if handle.is_null() {return 1;}let r=&mut *handle.cast::<ListReader>();
    if kind==3 {
        let mut value=[0;8];let code=r.session.next(kind,width,&mut value,||reader_byte(r.stream,r.source,r.length,&mut r.offset));
        if code==0 {*out=(i64::from_ne_bytes(value)!=0) as u8;}code
    } else {
        let length=if kind==4 {width}else{8};let out=core::slice::from_raw_parts_mut(out,length);
        r.session.next(kind,width,out,||reader_byte(r.stream,r.source,r.length,&mut r.offset))
    }
}
#[no_mangle]
pub unsafe extern "C" fn rfort_list_read_complex_next(handle:*mut core::ffi::c_void,width:usize,re:*mut u8,im:*mut u8)->i64 {
    if handle.is_null() {return 1;}let r=&mut *handle.cast::<ListReader>();let mut out=[0;16];
    let code=r.session.next(5,width,&mut out,||reader_byte(r.stream,r.source,r.length,&mut r.offset));
    if code==0 {core::ptr::copy_nonoverlapping(out.as_ptr(),re,8);core::ptr::copy_nonoverlapping(out.as_ptr().add(8),im,8);}code
}
#[no_mangle]
pub unsafe extern "C" fn rfort_list_read_end(handle:*mut core::ffi::c_void)->i64 {
    if handle.is_null() {return 1;}let r=&mut *handle.cast::<ListReader>();
    let code=r.session.finish(||reader_byte(r.stream,r.source,r.length,&mut r.offset));free(r.source.cast());core::ptr::drop_in_place(r);free(handle);code
}

/// A null stream denotes standard input. The destination holds `length` bytes
/// plus a terminator; the compiler copies it to user storage only on success.
#[no_mangle]
pub unsafe extern "C" fn rfort_read_record(
    stream: *mut core::ffi::c_void, destination: *mut u8, length: usize,
) -> i64 {
    read_record(stream,destination,length,false)
}
#[no_mangle]
pub unsafe extern "C" fn rfort_read_record_checked(stream:*mut core::ffi::c_void,destination:*mut u8,length:usize)->i64 {
    read_record(stream,destination,length,true)
}
unsafe fn read_record(stream:*mut core::ffi::c_void,destination:*mut u8,length:usize,checked:bool)->i64 {
    let stream = if stream.is_null() {
        #[cfg(windows)]
        { __acrt_iob_func(0) }
        #[cfg(target_os = "macos")]
        { __stdinp }
        #[cfg(not(any(windows, target_os = "macos")))]
        { stdin }
    } else { stream };
    let buffer = core::slice::from_raw_parts_mut(destination, length);
    let byte=|| {
        let value = fgetc(stream);
        if value >= 0 { Ok(Some(value as u8)) }
        else if ferror(stream) != 0 { Err(()) }
        else { Ok(None) }
    };
    let status=if checked {record_input::checked_record(buffer,byte)} else {record_input::record(buffer,byte)};
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
pub unsafe extern "C" fn rfort_character_compare(left:*const u8,left_length:usize,right:*const u8,right_length:usize)->i32 {
    character_input::compare(core::slice::from_raw_parts(left,left_length),core::slice::from_raw_parts(right,right_length))
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
unsafe fn free_output_list(pointer:*mut core::ffi::c_void) {
    let list=&*pointer.cast::<OutputList>();
    for i in 0..list.count {
        let item=&*list.items.add(i);
        if item.tag==4 {free(item.bits as usize as *mut core::ffi::c_void);}
    }
    free(list.items.cast());free(list.format.cast());free(pointer);
}
#[no_mangle]
pub unsafe extern "C" fn rfort_format_end(pointer:*mut core::ffi::c_void)->i32 {
    let list=&*pointer.cast::<OutputList>();
    rfort_formatted_write(list.format,list.length,list.items,list.count,list.advance);
    free_output_list(pointer);
    0
}
#[no_mangle]
pub unsafe extern "C" fn rfort_internal_format_end(pointer:*mut core::ffi::c_void,destination:*mut u8,length:usize)->i32 {
    let list=&*pointer.cast::<OutputList>();
    let format=core::slice::from_raw_parts(list.format,list.length);
    let mut parser=checked(GroupParser::new(format,list.count));
    while let Some(descriptor)=checked(parser.next()) {
        if descriptor.code==b'/' || descriptor.code==format_spec::REVERSION {fail("internal WRITE requires a single record; multiple records are not yet supported");}
    }
    core::ptr::write_bytes(destination,b' ',length);
    let mut cursor=0;
    render_format(list.format,list.length,list.items,list.count,&mut |byte| {
        if cursor==length {fail("internal WRITE record exceeds CHARACTER destination length");}
        *destination.add(cursor)=byte;
        cursor+=1;
    },true);
    *destination.add(length)=0;
    free_output_list(pointer);
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
/// INTEGER, REAL, or LOGICAL bits, with the kind in `length` (0 if unknown).
/// All storage remains live for this call.
unsafe fn render_format(
    format: *const u8,
    length: usize,
    items: *const Item,
    count: usize,
    emit: &mut impl FnMut(u8),
    advancing: bool,
) {
    let bytes = core::slice::from_raw_parts(format, length);
    let items = core::slice::from_raw_parts(items, count);
    let value = |i: usize, f: &mut dyn FnMut(format_spec::Value<'_>)| {
        let item = &items[i];
        f(match item.tag {
            1 => {
                let v = item.bits as i64;
                let kind = if item.length != 0 { item.length as u8 } else if v == v as i32 as i64 { 4 } else { 8 };
                format_spec::Value::Integer(v, kind)
            }
            2 => format_spec::Value::Real(f64::from_bits(item.bits), if item.length == 4 { 4 } else { 8 }),
            3 => format_spec::Value::Logical(item.bits != 0),
            _ => format_spec::Value::Character(core::slice::from_raw_parts(item.bits as usize as *const u8, item.length as usize)),
        })
    };
    checked(format_spec::render_with(bytes, count, &value, &mut |b| emit(b), advancing));
}

#[no_mangle]
pub unsafe extern "C" fn rfort_formatted_write(format:*const u8,length:usize,items:*const Item,count:usize,advance:i32) {
    render_format(format,length,items,count,&mut |byte| {putchar(byte as i32);},advance!=0);
    if advance!=0 {putchar(10);} else {fflush(core::ptr::null_mut());}
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

/// Convert an already rounded real value only when representable in its result kind.
#[no_mangle]
pub unsafe extern "C" fn rfort_checked_integer(value:f64,kind:i64,ceiling:i64)->i64 {
    let limit=match kind {1=>128.0,2=>32768.0,4=>2147483648.0,_=>9223372036854775808.0};
    if !value.is_finite() || value < -limit || value >= limit {
        extern "C" {fn exit(code:i32)->!;}
        let message=if ceiling==0 {b"Runtime error: FLOOR result is outside INTEGER range\n" as &[u8]} else {b"Runtime error: CEILING result is outside INTEGER range\n" as &[u8]};
        #[cfg(windows)] let stream=__acrt_iob_func(2);
        #[cfg(target_os="macos")] let stream={extern "C" {static mut __stderrp:*mut core::ffi::c_void;} __stderrp};
        #[cfg(not(any(windows,target_os="macos")))] let stream={extern "C" {static mut stderr:*mut core::ffi::c_void;} stderr};
        fwrite(message.as_ptr(),1,message.len(),stream);
        exit(1);
    }
    value as i64
}

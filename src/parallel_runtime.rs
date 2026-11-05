// SPDX-License-Identifier: MIT
//! Synchronous fork/join over a persistent, bounded native worker pool.
//! Only compiler-proven, non-nested kernels enter here. No Fortran storage
//! survives the call: completion uses release/acquire before the caller returns.
use core::{
    cell::UnsafeCell,
    ffi::c_void,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering::*},
};
// Callback arguments: captures, inclusive first/last indices, worker index.
type Kernel = unsafe extern "C" fn(*mut c_void, i64, i64, i64);
#[derive(Clone, Copy)]
struct Job {
    kernel: Option<Kernel>,
    context: *mut c_void,
    start: i64,
    end: i64,
    threads: usize,
}
struct Shared(UnsafeCell<Job>);
unsafe impl Sync for Shared {}
static JOB: Shared = Shared(UnsafeCell::new(Job {
    kernel: None,
    context: core::ptr::null_mut(),
    start: 0,
    end: -1,
    threads: 1,
}));
static LOCK: AtomicBool = AtomicBool::new(false);
static EPOCH: AtomicUsize = AtomicUsize::new(0);
static DONE: AtomicUsize = AtomicUsize::new(0);
static WORKERS: AtomicUsize = AtomicUsize::new(0);
static STARTED: AtomicBool = AtomicBool::new(false);
extern "C" {
    fn getenv(name: *const u8) -> *const u8;
}
fn requested() -> usize {
    unsafe {
        let mut p = getenv(b"RFORT_NUM_THREADS\0".as_ptr());
        if p.is_null() {
            return 4;
        }
        let mut n = 0usize;
        while *p != 0 {
            if !(*p).is_ascii_digit() {
                return 4;
            }
            n = n.saturating_mul(10).saturating_add((*p - b'0') as usize);
            p = p.add(1);
        }
        n.clamp(1, 64)
    }
}
#[cfg(windows)]
fn pause() {
    unsafe {
        extern "system" {
            fn Sleep(ms: u32);
        }
        Sleep(1);
    }
}
#[cfg(unix)]
fn pause() {
    unsafe {
        #[repr(C)]
        struct Timespec {
            seconds: isize,
            nanos: isize,
        }
        extern "C" {
            fn nanosleep(request: *const Timespec, remainder: *mut Timespec) -> i32;
        }
        nanosleep(
            &Timespec {
                seconds: 0,
                nanos: 1_000_000,
            },
            core::ptr::null_mut(),
        );
    }
}
// Auto-reset events avoid the coarse timer tick of Sleep(1) on Windows.
#[cfg(windows)]
mod events {
    use super::*;
    static HANDLES: [AtomicUsize; 64] = [const { AtomicUsize::new(0) }; 64];
    extern "system" {
        fn CreateEventA(
            attrs: *const c_void,
            manual: i32,
            initial: i32,
            name: *const u8,
        ) -> *mut c_void;
        fn SetEvent(handle: *mut c_void) -> i32;
        fn ResetEvent(handle: *mut c_void) -> i32;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    fn handle(id: usize) -> *mut c_void {
        HANDLES[id].load(Relaxed) as *mut c_void
    }
    pub unsafe fn init(id: usize) -> bool {
        let h = CreateEventA(core::ptr::null(), 0, 0, core::ptr::null());
        HANDLES[id].store(h as usize, Relaxed);
        !h.is_null()
    }
    pub unsafe fn close(id: usize) {
        CloseHandle(handle(id));
        HANDLES[id].store(0, Relaxed);
    }
    pub unsafe fn signal(id: usize) {
        SetEvent(handle(id));
    }
    pub unsafe fn reset(id: usize) {
        ResetEvent(handle(id));
    }
    pub unsafe fn wait(id: usize) {
        WaitForSingleObject(handle(id), u32::MAX);
    }
}
unsafe fn portion(id: usize, job: Job) {
    let count = (job.end as i128 - job.start as i128 + 1).max(0);
    let first = count * id as i128 / job.threads as i128;
    let last = count * (id + 1) as i128 / job.threads as i128;
    if first < last {
        (job.kernel.unwrap())(
            job.context,
            (job.start as i128 + first) as i64,
            (job.start as i128 + last - 1) as i64,
            id as i64,
        );
    }
}
unsafe fn worker(id: usize) {
    let mut seen = 0;
    loop {
        let epoch = EPOCH.load(Acquire);
        if epoch == seen {
            #[cfg(windows)]
            events::wait(id);
            #[cfg(unix)]
            pause();
            continue;
        }
        seen = epoch;
        let job = *JOB.0.get();
        portion(id, job);
        let finished = DONE.fetch_add(1, Release) + 1;
        #[cfg(windows)]
        if finished == job.threads - 1 {
            events::signal(0);
        }
        #[cfg(unix)]
        let _ = finished;
    }
}
#[cfg(windows)]
unsafe fn spawn(id: usize) -> bool {
    unsafe extern "system" fn entry(id: *mut c_void) -> u32 {
        worker(id as usize);
        0
    }
    extern "system" {
        fn CreateThread(
            attrs: *const c_void,
            size: usize,
            entry: unsafe extern "system" fn(*mut c_void) -> u32,
            arg: *mut c_void,
            flags: u32,
            id: *mut u32,
        ) -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    if !events::init(id) {
        return false;
    }
    let handle = CreateThread(
        core::ptr::null(),
        0,
        entry,
        id as *mut c_void,
        0,
        core::ptr::null_mut(),
    );
    if handle.is_null() {
        events::close(id);
        false
    } else {
        CloseHandle(handle);
        true
    }
}
#[cfg(unix)]
unsafe fn spawn(id: usize) -> bool {
    unsafe extern "C" fn entry(id: *mut c_void) -> *mut c_void {
        worker(id as usize);
        core::ptr::null_mut()
    }
    extern "C" {
        fn pthread_create(
            thread: *mut usize,
            attrs: *const c_void,
            entry: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
            arg: *mut c_void,
        ) -> i32;
        fn pthread_detach(thread: usize) -> i32;
    }
    let mut thread = 0;
    if pthread_create(&mut thread, core::ptr::null(), entry, id as *mut c_void) != 0 {
        false
    } else {
        pthread_detach(thread);
        true
    }
}
#[no_mangle]
pub unsafe extern "C" fn rfort_parallel_for(
    kernel: Kernel,
    context: *mut c_void,
    start: i64,
    end: i64,
) -> i32 {
    // Empty ranges and tiny kernels avoid thread-pool startup entirely.
    if end < start {
        return 0;
    }
    if end as i128 - start as i128 + 1 < 4096 || requested() == 1 {
        kernel(context, start, end, 0);
        return 0;
    }
    while LOCK
        .compare_exchange(false, true, Acquire, Relaxed)
        .is_err()
    {
        pause();
    }
    if !STARTED.load(Relaxed) {
        let mut count = 0;
        #[cfg(windows)]
        let ready = events::init(0);
        #[cfg(unix)]
        let ready = true;
        for id in 1..if ready { requested() } else { 1 } {
            if !spawn(id) {
                break;
            }
            count += 1;
        }
        WORKERS.store(count, Relaxed);
        STARTED.store(true, Relaxed);
    }
    let workers = WORKERS.load(Relaxed);
    let job = Job {
        kernel: Some(kernel),
        context,
        start,
        end,
        threads: workers + 1,
    };
    *JOB.0.get() = job;
    DONE.store(0, Relaxed);
    #[cfg(windows)]
    if workers > 0 {
        events::reset(0);
    }
    EPOCH.fetch_add(1, Release);
    #[cfg(windows)]
    for id in 1..=workers {
        events::signal(id);
    }
    portion(0, job);
    while DONE.load(Acquire) != workers {
        #[cfg(windows)]
        events::wait(0);
        #[cfg(unix)]
        pause();
    }
    #[cfg(not(test))]
    if !getenv(b"RFORT_PARALLEL_TRACE\0".as_ptr()).is_null() {
        extern "C" {
            fn fprintf(stream: *mut c_void, format: *const u8, ...) -> i32;
        }
        fprintf(
            crate::rfort_stderr_stream(),
            b"rfort parallel: %d threads\n\0".as_ptr(),
            (workers + 1) as i32,
        );
    }
    LOCK.store(false, Release);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct Probe {
        hits: Vec<AtomicUsize>,
        threads: Mutex<std::collections::HashSet<std::thread::ThreadId>>,
    }
    unsafe extern "C" fn fill(context: *mut c_void, first: i64, last: i64, _worker: i64) {
        let probe = &*(context as *const Probe);
        probe
            .threads
            .lock()
            .unwrap()
            .insert(std::thread::current().id());
        for i in first..=last {
            probe.hits[(i + 5000) as usize].fetch_add(1, Relaxed);
        }
    }
    #[test]
    fn partitions_repeated_jobs_and_concurrent_callers() {
        std::env::set_var("RFORT_NUM_THREADS", "4");
        let run = || {
            let mut probe = Probe {
                hits: (0..10001).map(|_| AtomicUsize::new(0)).collect(),
                threads: Mutex::new(Default::default()),
            };
            for _ in 0..20 {
                unsafe {
                    rfort_parallel_for(fill, (&mut probe as *mut Probe).cast(), -5000, 5000);
                }
            }
            assert!(probe.hits.iter().all(|n| n.load(Relaxed) == 20));
            assert_eq!(probe.threads.lock().unwrap().len(), 4);
            unsafe {
                rfort_parallel_for(fill, (&mut probe as *mut Probe).cast(), 1, 0);
            }
            assert!(probe.hits.iter().all(|n| n.load(Relaxed) == 20));
        };
        std::thread::scope(|s| {
            s.spawn(run);
            s.spawn(run);
        });
    }
}

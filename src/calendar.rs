// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! One local civil-time snapshot, shared by interpreter and native runtime.
#[cfg(target_os = "windows")]
mod os {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        weekday: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    impl FileTime {
        fn ticks(&self) -> i64 {
            ((u64::from(self.high) << 32) | u64::from(self.low)) as i64
        }
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemTime(time: *mut SystemTime);
        fn SystemTimeToTzSpecificLocalTime(
            zone: *const core::ffi::c_void,
            utc: *const SystemTime,
            local: *mut SystemTime,
        ) -> i32;
        fn SystemTimeToFileTime(time: *const SystemTime, file: *mut FileTime) -> i32;
    }
    pub fn read() -> Option<[i64; 8]> {
        let (mut utc, mut local) = (SystemTime::default(), SystemTime::default());
        let (mut utc_file, mut local_file) = (FileTime::default(), FileTime::default());
        unsafe {
            GetSystemTime(&mut utc);
            if SystemTimeToTzSpecificLocalTime(core::ptr::null(), &utc, &mut local) == 0
                || SystemTimeToFileTime(&utc, &mut utc_file) == 0
                || SystemTimeToFileTime(&local, &mut local_file) == 0
            {
                return None;
            }
        }
        Some([
            i64::from(local.year),
            i64::from(local.month),
            i64::from(local.day),
            (local_file.ticks() - utc_file.ticks()) / 600_000_000,
            i64::from(local.hour),
            i64::from(local.minute),
            i64::from(local.second),
            i64::from(local.millis),
        ])
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod os {
    use core::ffi::{c_char, c_int, c_long};
    #[repr(C)]
    struct Timespec {
        seconds: c_long,
        nanos: c_long,
    }
    #[repr(C)]
    struct Tm {
        second: c_int,
        minute: c_int,
        hour: c_int,
        day: c_int,
        month: c_int,
        year: c_int,
        weekday: c_int,
        yearday: c_int,
        dst: c_int,
        offset: c_long,
        zone: *const c_char,
    }
    extern "C" {
        fn clock_gettime(clock: c_int, time: *mut Timespec) -> c_int;
        fn localtime_r(time: *const c_long, result: *mut Tm) -> *mut Tm;
    }
    pub fn read() -> Option<[i64; 8]> {
        let mut time = Timespec {
            seconds: 0,
            nanos: 0,
        };
        let mut local = core::mem::MaybeUninit::<Tm>::uninit();
        unsafe {
            if clock_gettime(0, &mut time) != 0
                || localtime_r(&time.seconds, local.as_mut_ptr()).is_null()
            {
                return None;
            }
            let local = local.assume_init();
            Some([
                i64::from(local.year) + 1900,
                i64::from(local.month) + 1,
                i64::from(local.day),
                local.offset as i64 / 60,
                i64::from(local.hour),
                i64::from(local.minute),
                i64::from(local.second),
                time.nanos as i64 / 1_000_000,
            ])
        }
    }
}
pub fn snapshot() -> [i64; 8] {
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    {
        os::read().unwrap_or([-i64::from(i32::MAX); 8])
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        [-i64::from(i32::MAX); 8]
    }
}
/// The compiler passes a writable eight-element INTEGER storage buffer.
#[no_mangle]
pub unsafe extern "C" fn rfort_date_snapshot(destination: *mut i64) -> i64 {
    core::ptr::copy_nonoverlapping(snapshot().as_ptr(), destination, 8);
    0
}

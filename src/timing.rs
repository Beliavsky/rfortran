// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! OS clocks shared by the interpreter and the no_std native runtime.
#[cfg(target_os = "windows")]
mod os {
    use core::ffi::c_void;
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    impl FileTime {
        fn ticks(&self) -> u64 {
            (u64::from(self.high) << 32) | u64::from(self.low)
        }
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn GetProcessTimes(
            process: *mut c_void,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn QueryPerformanceCounter(value: *mut i64) -> i32;
        fn QueryPerformanceFrequency(value: *mut i64) -> i32;
    }
    pub fn cpu() -> f64 {
        let (mut creation, mut exit, mut kernel, mut user) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        if unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        } == 0
        {
            return -1.0;
        }
        (kernel.ticks() as f64 + user.ticks() as f64) / 10_000_000.0
    }
    pub fn monotonic(rate: i64) -> Option<i128> {
        let (mut count, mut frequency) = (0, 0);
        if unsafe { QueryPerformanceCounter(&mut count) } == 0
            || unsafe { QueryPerformanceFrequency(&mut frequency) } == 0
            || frequency <= 0
            || count < 0
        {
            return None;
        }
        Some(i128::from(count) * i128::from(rate) / i128::from(frequency))
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod os {
    use core::ffi::{c_int, c_long};
    #[repr(C)]
    struct Timespec {
        seconds: c_long,
        nanos: c_long,
    }
    extern "C" {
        fn clock_gettime(clock: c_int, time: *mut Timespec) -> c_int;
    }
    #[cfg(target_os = "linux")]
    const CLOCKS: (c_int, c_int) = (1, 2);
    #[cfg(target_os = "macos")]
    const CLOCKS: (c_int, c_int) = (6, 12);
    fn read(clock: c_int) -> Option<Timespec> {
        let mut time = Timespec {
            seconds: 0,
            nanos: 0,
        };
        if unsafe { clock_gettime(clock, &mut time) } == 0 {
            Some(time)
        } else {
            None
        }
    }
    pub fn cpu() -> f64 {
        read(CLOCKS.1).map_or(-1.0, |t| {
            t.seconds as f64 + t.nanos as f64 / 1_000_000_000.0
        })
    }
    pub fn monotonic(rate: i64) -> Option<i128> {
        read(CLOCKS.0).map(|t| {
            i128::from(t.seconds) * i128::from(rate)
                + i128::from(t.nanos) * i128::from(rate) / 1_000_000_000
        })
    }
}
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod os {
    pub fn cpu() -> f64 {
        -1.0
    }
    pub fn monotonic(_: i64) -> Option<i128> {
        None
    }
}
pub fn rate(kind: i64) -> i64 {
    if kind == 8 {
        1_000_000_000
    } else {
        1000
    }
}
pub fn maximum(kind: i64) -> i64 {
    if kind == 8 {
        i64::MAX
    } else {
        i64::from(i32::MAX)
    }
}
fn wrap(ticks: Option<i128>, kind: i64) -> i64 {
    let maximum = maximum(kind);
    ticks.map_or(-maximum, |n| n.rem_euclid(i128::from(maximum) + 1) as i64)
}
#[no_mangle]
pub extern "C" fn rfort_cpu_seconds() -> f64 {
    os::cpu()
}
#[no_mangle]
pub extern "C" fn rfort_clock_count(kind: i64) -> i64 {
    wrap(os::monotonic(rate(kind)), kind)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn count_wrap_and_unavailable() {
        for kind in [4, 8] {
            let max = maximum(kind);
            assert_eq!(wrap(Some(i128::from(max)), kind), max);
            assert_eq!(wrap(Some(i128::from(max) + 1), kind), 0);
            assert_eq!(wrap(Some(i128::from(max) + 2), kind), 1);
            assert_eq!(wrap(None, kind), -max);
            let count = rfort_clock_count(kind);
            assert!(count >= 0 && count <= max);
        }
        assert!(rfort_cpu_seconds() >= 0.0);
    }
}

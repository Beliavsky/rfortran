// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Opt-in compiler pass timings on stderr: RFORT_TIME_PASSES=1.
use std::{sync::OnceLock, time::Instant};

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("RFORT_TIME_PASSES").is_some_and(|v| v == "1"))
}

pub(crate) struct Pass<'a> {
    name: &'a str,
    start: Option<Instant>,
}

impl<'a> Pass<'a> {
    pub(crate) fn new(name: &'a str) -> Self {
        let start = enabled().then(|| {
            eprintln!("[pass] start {name}");
            Instant::now()
        });
        Self { name, start }
    }
}

impl Drop for Pass<'_> {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            eprintln!("[pass] end {}: {:.3}s", self.name, start.elapsed().as_secs_f64());
        }
    }
}

pub(crate) fn run<T>(name: &str, f: impl FnOnce() -> T) -> T {
    let _pass = Pass::new(name);
    f()
}

pub(crate) struct Work {
    name: &'static str,
    start: Option<Instant>,
}

impl Work {
    pub(crate) fn new(name: &'static str) -> Self {
        Self { name, start: enabled().then(Instant::now) }
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let Some(start) = self.start else { return };
        struct Totals {
            last: Instant,
            values: std::collections::BTreeMap<&'static str, (u64, f64, f64)>,
        }
        thread_local! {
            static TOTALS: std::cell::RefCell<Totals> = std::cell::RefCell::new(Totals {
                last: Instant::now(), values: Default::default(),
            });
        }
        TOTALS.with(|totals| {
            let mut totals = totals.borrow_mut();
            let seconds = start.elapsed().as_secs_f64();
            let entry = totals.values.entry(self.name).or_default();
            entry.0 += 1;
            entry.1 += seconds;
            entry.2 = entry.2.max(seconds);
            if totals.last.elapsed().as_secs() >= 2 {
                for (name, (calls, total, max)) in &totals.values {
                    eprintln!("[work] {name}: calls={calls} total={total:.3}s max={max:.3}s");
                }
                totals.last = Instant::now();
            }
        });
    }
}

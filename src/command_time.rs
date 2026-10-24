// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Concise wall-clock timings, independent of detailed pass profiling.
use std::time::Instant;

pub struct Overall(Option<Instant>);
impl Overall {
    pub fn new(enabled: bool, started: Instant) -> Self {
        Self(enabled.then_some(started))
    }
}
impl Drop for Overall {
    fn drop(&mut self) {
        if let Some(started) = self.0 {
            eprintln!("Overall:     {:.3} s", started.elapsed().as_secs_f64());
        }
    }
}

pub struct Phase {
    started: Option<Instant>,
    name: &'static str,
}
impl Phase {
    pub fn new(enabled: bool, name: &'static str) -> Self {
        Self { started: enabled.then(Instant::now), name }
    }
    pub fn finish(&mut self, success: bool) {
        if let Some(started) = self.started.take() {
            eprintln!("{}: {:.3} s{}", self.name, started.elapsed().as_secs_f64(),
                if success { "" } else { " (failed)" });
        }
    }
}
impl Drop for Phase {
    fn drop(&mut self) { self.finish(false); }
}

// Return through the timing guards while preserving the child's exit code.
#[derive(Debug)]
pub struct ProgramExit(pub i32);
impl std::fmt::Display for ProgramExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "program exited with status {}", self.0)
    }
}
impl std::error::Error for ProgramExit {}

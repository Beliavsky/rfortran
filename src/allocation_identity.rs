// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Allocation identities survive MOVE_ALLOC and are never reused.
use core::sync::atomic::{AtomicI64, Ordering};

#[no_mangle]
pub extern "C" fn rfort_allocation_identity() -> i64 {
    static NEXT: AtomicI64 = AtomicI64::new(1);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .unwrap_or(0)
}

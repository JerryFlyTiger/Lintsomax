// SPDX-License-Identifier: GPL-3.0-or-later WITH Linux-syscall-note
//! Physical frame allocator: a bump allocator growing upward from `__kernel_end`.
//!
//! RAM on QEMU virt starts at 0x4000_0000 and spans 128 MiB, up to 0x4800_0000.
//! There is no free: M1 only has to hand out page-table and kernel data frames,
//! and never reclaims them.

use core::sync::atomic::{AtomicU64, Ordering};

const FRAME_SIZE: u64 = 4096;
const RAM_END: u64 = 0x4800_0000; // 128 MiB, counted from 0x4000_0000 on QEMU virt

extern "C" {
    static __kernel_end: u8;
}

static NEXT_FRAME: AtomicU64 = AtomicU64::new(0);
static TOTAL_FRAMES: AtomicU64 = AtomicU64::new(0);
static USED_FRAMES: AtomicU64 = AtomicU64::new(0);

/// Initialise the allocator. The start is `__kernel_end`, which the linker
/// script has already aligned to 4096.
pub fn init() {
    let start = unsafe { &__kernel_end as *const u8 as u64 };
    debug_assert_eq!(start % FRAME_SIZE, 0);
    NEXT_FRAME.store(start, Ordering::Relaxed);
    TOTAL_FRAMES.store((RAM_END - start) / FRAME_SIZE, Ordering::Relaxed);
    USED_FRAMES.store(0, Ordering::Relaxed);
}

/// Allocate one 4096-aligned physical frame, zero it, and return its physical
/// address. Returns `None` once RAM is exhausted.
pub fn alloc_frame() -> Option<u64> {
    let addr = NEXT_FRAME.fetch_add(FRAME_SIZE, Ordering::Relaxed);
    if addr + FRAME_SIZE > RAM_END {
        return None;
    }
    USED_FRAMES.fetch_add(1, Ordering::Relaxed);

    unsafe {
        core::ptr::write_bytes(addr as *mut u8, 0, FRAME_SIZE as usize);
    }

    Some(addr)
}

/// Returns `(used frames, total frames)`.
pub fn stats() -> (u64, u64) {
    (
        USED_FRAMES.load(Ordering::Relaxed),
        TOTAL_FRAMES.load(Ordering::Relaxed),
    )
}

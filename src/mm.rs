// SPDX-License-Identifier: GPL-3.0-or-later WITH Linux-syscall-note
//! 實體記憶體框配置器：一個從 `__kernel_end` 開始往上長的 bump allocator。
//!
//! QEMU virt 的 RAM 從 0x4000_0000 起，共 128 MiB，到 0x4800_0000。
//! 不支援 free：M1 只需要把頁表框和核心資料框配出去，之後不收回。

use core::sync::atomic::{AtomicU64, Ordering};

const FRAME_SIZE: u64 = 4096;
const RAM_END: u64 = 0x4800_0000; // 128 MiB，QEMU virt 的 RAM 從 0x4000_0000 起算

extern "C" {
    static __kernel_end: u8;
}

static NEXT_FRAME: AtomicU64 = AtomicU64::new(0);
static TOTAL_FRAMES: AtomicU64 = AtomicU64::new(0);
static USED_FRAMES: AtomicU64 = AtomicU64::new(0);

/// 初始化配置器：起點是 `__kernel_end`（已由 linker script 對齊到 4096）。
pub fn init() {
    let start = unsafe { &__kernel_end as *const u8 as u64 };
    debug_assert_eq!(start % FRAME_SIZE, 0);
    NEXT_FRAME.store(start, Ordering::Relaxed);
    TOTAL_FRAMES.store((RAM_END - start) / FRAME_SIZE, Ordering::Relaxed);
    USED_FRAMES.store(0, Ordering::Relaxed);
}

/// 配一個 4096 對齊的實體框，回傳實體位址，並把內容清零。
/// RAM 用盡時回傳 `None`。
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

/// 回傳 `(已用框數, 總框數)`。
pub fn stats() -> (u64, u64) {
    (
        USED_FRAMES.load(Ordering::Relaxed),
        TOTAL_FRAMES.load(Ordering::Relaxed),
    )
}

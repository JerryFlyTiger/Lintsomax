//! 4 階頁表（4 KiB granule、48-bit VA）與 MMU 開關。
//!
//! M1 只建恆等映射（VA == PA）：TTBR0_EL1 涵蓋整個核心看得到的位址空間，
//! 之後的里程碑再談使用者位址空間與 TTBR1。

use crate::mm;
use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

extern "C" {
    static __text_start: u8;
    static __text_end: u8;
    static __rodata_start: u8;
    static __rodata_end: u8;
    static __data_start: u8;
    static __kernel_end: u8;
}

const RAM_START: u64 = 0x4000_0000;
const RAM_END: u64 = 0x4800_0000; // 128 MiB
const PAGE_SIZE: u64 = 4096;
const BLOCK_2M: u64 = 0x20_0000;

// 描述子型別（bits[1:0]）
const TABLE_DESC: u64 = 0b11;
const BLOCK_DESC: u64 = 0b01;
const PAGE_DESC: u64 = 0b11;

// PA 遮罩
const PA_MASK: u64 = 0x0000_FFFF_FFFF_F000; // bits[47:12]，table/page 描述子用
const PA_MASK_2M: u64 = 0x0000_FFFF_FFE0_0000; // bits[47:21]，L2 block
const PA_MASK_1G: u64 = 0x0000_FFFF_C000_0000; // bits[47:30]，L1 block

const MAIR_NORMAL_IDX: u64 = 0;
const MAIR_DEVICE_IDX: u64 = 1;
const AP_RW: u64 = 0b00;
const AP_RO: u64 = 0b10;
const SH_INNER: u64 = 0b11;
const SH_NONE: u64 = 0b00;

/// 頁表本身存放的位置：`mm` 配的框。啟用 MMU 後這段記憶體也在
/// `__kernel_end..RAM_END` 的恆等映射範圍內，所以走訪頁表在開關 MMU
/// 前後都用同一份程式碼。
type Table = [u64; 512];

static ROOT: AtomicU64 = AtomicU64::new(0);

fn attrs(mair_idx: u64, ap: u64, sh: u64, pxn: u64, uxn: u64) -> u64 {
    (mair_idx << 2) | (ap << 6) | (sh << 8) | (1 << 10) /* AF */ | (pxn << 53) | (uxn << 54)
}

fn device_rw() -> u64 {
    attrs(MAIR_DEVICE_IDX, AP_RW, SH_NONE, 1, 1)
}

fn normal_rw() -> u64 {
    attrs(MAIR_NORMAL_IDX, AP_RW, SH_INNER, 1, 1)
}

fn normal_ro_exec() -> u64 {
    // 核心自己要能執行 .text，所以 PXN=0；EL0 還不存在，UXN=1 保守設定。
    attrs(MAIR_NORMAL_IDX, AP_RO, SH_INNER, 0, 1)
}

fn normal_ro_noexec() -> u64 {
    attrs(MAIR_NORMAL_IDX, AP_RO, SH_INNER, 1, 1)
}

unsafe fn table_ref(pa: u64) -> &'static mut Table {
    &mut *(pa as *mut Table)
}

/// 取得 `entry` 指向的下一階表；不存在就配一個新的框並設成 table descriptor。
/// 若 `entry` 已經是別種描述子（block），代表呼叫端邏輯衝突，直接 panic。
unsafe fn get_or_create(entry: &mut u64) -> &'static mut Table {
    if *entry & 1 == 0 {
        let pa = mm::alloc_frame().expect("paging: 記憶體用盡，配不出新的頁表框");
        *entry = pa | TABLE_DESC;
        table_ref(pa)
    } else {
        assert_eq!(*entry & 0b11, 0b11, "paging: 預期是 table descriptor，卻撞到 block");
        table_ref(*entry & PA_MASK)
    }
}

unsafe fn map_page(l0: &mut Table, va: u64, pa: u64, attr_bits: u64) {
    let l1 = get_or_create(&mut l0[((va >> 39) & 0x1ff) as usize]);
    let l2 = get_or_create(&mut l1[((va >> 30) & 0x1ff) as usize]);
    let l3 = get_or_create(&mut l2[((va >> 21) & 0x1ff) as usize]);
    l3[((va >> 12) & 0x1ff) as usize] = (pa & PA_MASK) | PAGE_DESC | attr_bits;
}

unsafe fn map_block_2m(l0: &mut Table, va: u64, pa: u64, attr_bits: u64) {
    let l1 = get_or_create(&mut l0[((va >> 39) & 0x1ff) as usize]);
    let l2 = get_or_create(&mut l1[((va >> 30) & 0x1ff) as usize]);
    l2[((va >> 21) & 0x1ff) as usize] = (pa & PA_MASK_2M) | BLOCK_DESC | attr_bits;
}

unsafe fn map_block_1g(l0: &mut Table, va: u64, pa: u64, attr_bits: u64) {
    let l1 = get_or_create(&mut l0[((va >> 39) & 0x1ff) as usize]);
    l1[((va >> 30) & 0x1ff) as usize] = (pa & PA_MASK_1G) | BLOCK_DESC | attr_bits;
}

/// 建好整份恆等映射頁表，回傳給 `enable()` 用的 TTBR0_EL1 值（L0 表的實體位址）。
pub fn init() -> u64 {
    unsafe {
        let l0_pa = mm::alloc_frame().expect("paging: 配不出 L0 表");
        ROOT.store(l0_pa, Ordering::Relaxed);
        let l0 = table_ref(l0_pa);

        // 0x0000_0000 - 0x4000_0000：MMIO（含 PL011 UART），1 GiB block。
        map_block_1g(l0, 0, 0, device_rw());

        let text_start = &__text_start as *const u8 as u64;
        let text_end = &__text_end as *const u8 as u64;
        let rodata_start = &__rodata_start as *const u8 as u64;
        let rodata_end = &__rodata_end as *const u8 as u64;
        let data_start = &__data_start as *const u8 as u64;
        let kernel_end = &__kernel_end as *const u8 as u64;

        // 核心載入點之前這段（RAM 起點 .. __text_start）也要是 Normal RW，
        // 不然開機早期會踩到還沒映射的資料。
        let mut va = RAM_START;
        while va < text_start {
            map_page(l0, va, va, normal_rw());
            va += PAGE_SIZE;
        }

        let mut va = text_start;
        while va < text_end {
            map_page(l0, va, va, normal_ro_exec());
            va += PAGE_SIZE;
        }

        let mut va = rodata_start;
        while va < rodata_end {
            map_page(l0, va, va, normal_ro_noexec());
            va += PAGE_SIZE;
        }

        let mut va = data_start;
        while va < kernel_end {
            map_page(l0, va, va, normal_rw());
            va += PAGE_SIZE;
        }

        // __kernel_end .. RAM_END：優先用 2 MiB block，收尾對不齊的部分退回 4K 頁。
        let mut va = kernel_end;
        while va < RAM_END {
            if va.is_multiple_of(BLOCK_2M) && va + BLOCK_2M <= RAM_END {
                map_block_2m(l0, va, va, normal_rw());
                va += BLOCK_2M;
            } else {
                map_page(l0, va, va, normal_rw());
                va += PAGE_SIZE;
            }
        }

        l0_pa
    }
}

/// 開啟 MMU：設定 MAIR/TCR/TTBR0，然後翻 SCTLR_EL1 的 M/C/I 位元。
pub fn enable(ttbr0: u64) {
    unsafe {
        // 先清乾淨可能殘留的 TLB。
        asm!("tlbi vmalle1", "dsb ish", "isb", options(nostack));

        let mair: u64 = 0x0000_0000_0000_04FF;
        asm!("msr mair_el1, {v}", v = in(reg) mair, options(nostack));

        let mmfr0: u64;
        asm!("mrs {v}, id_aa64mmfr0_el1", v = out(reg) mmfr0, options(nomem, nostack));
        let parange = mmfr0 & 0xF;
        let ips = if parange > 0b101 { 0b101 } else { parange };

        let tcr: u64 = 16                 // T0SZ, bits[5:0]
            | (16 << 16)                  // T1SZ, bits[21:16]
            | (0b01 << 8)                 // IRGN0, bits[9:8]
            | (0b01 << 10)                // ORGN0, bits[11:10]
            | (0b11 << 12)                // SH0, bits[13:12]
            // TG0 = 4KiB 的編碼就是 0b00（bits[15:14]），刻意不寫成 `| (0b00 << 14)`
            | (1 << 23)                   // EPD1：不使用 TTBR1，明確停用上半部表走訪
            | (0b01 << 24)                // IRGN1, bits[25:24]
            | (0b01 << 26)                // ORGN1, bits[27:26]
            | (0b11 << 28)                // SH1, bits[29:28]
            | (0b10 << 30)                // TG1 = 4KiB（TTBR1 編碼不同！）, bits[31:30]
            | (ips << 32); // IPS, bits[34:32]
        asm!("msr tcr_el1, {v}", v = in(reg) tcr, options(nostack));

        asm!("msr ttbr0_el1, {v}", v = in(reg) ttbr0, options(nostack));

        asm!("dsb ish", "isb", options(nostack));

        let mut sctlr: u64;
        asm!("mrs {v}, sctlr_el1", v = out(reg) sctlr, options(nomem, nostack));
        sctlr |= (1 << 0) | (1 << 2) | (1 << 12); // M | C | I
        asm!("msr sctlr_el1, {v}", v = in(reg) sctlr, options(nostack));
        asm!("isb", options(nostack));
    }
}

/// 軟體走訪頁表，把 VA 轉成 PA；純粹用來驗證映射對不對，不影響硬體狀態。
pub fn translate(va: u64) -> Option<u64> {
    let l0_pa = ROOT.load(Ordering::Relaxed);
    if l0_pa == 0 {
        return None;
    }
    unsafe {
        let l0 = table_ref(l0_pa);
        let e0 = l0[((va >> 39) & 0x1ff) as usize];
        if e0 & 1 == 0 {
            return None;
        }
        let l1 = table_ref(e0 & PA_MASK);
        let e1 = l1[((va >> 30) & 0x1ff) as usize];
        if e1 & 1 == 0 {
            return None;
        }
        if e1 & 0b11 == BLOCK_DESC {
            return Some((e1 & PA_MASK_1G) | (va & 0x3FFF_FFFF));
        }
        let l2 = table_ref(e1 & PA_MASK);
        let e2 = l2[((va >> 21) & 0x1ff) as usize];
        if e2 & 1 == 0 {
            return None;
        }
        if e2 & 0b11 == BLOCK_DESC {
            return Some((e2 & PA_MASK_2M) | (va & 0x1F_FFFF));
        }
        let l3 = table_ref(e2 & PA_MASK);
        let e3 = l3[((va >> 12) & 0x1ff) as usize];
        if e3 & 0b11 != PAGE_DESC {
            return None;
        }
        Some((e3 & PA_MASK) | (va & 0xFFF))
    }
}

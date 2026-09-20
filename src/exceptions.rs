// SPDX-License-Identifier: GPL-3.0-or-later WITH Linux-syscall-note
//! 例外向量表與故障報告。
//!
//! 這是 Lintsomax 的主題所在：故障不該是「整台機器倒下」，
//! 而該是「一份看得懂的報告」。M0 先做到報告，隔離留給後面的里程碑。

use core::arch::{asm, global_asm};

// 16 個向量入口，每個 128 bytes（.align 7），整張表 2048 對齊（.align 11）。
// 每個入口只做兩件事：把自己的編號放進 x0，跳到共用處理常式。
global_asm!(
    ".macro VEC_ENTRY idx",
    "   .align 7",
    "   mov x0, #\\idx",
    "   b   __exception_common",
    ".endm",

    ".section .text",
    ".align 11",
    ".global __vectors",
    "__vectors:",
    // 目前 EL，使用 SP_EL0
    "   VEC_ENTRY 0", // Synchronous
    "   VEC_ENTRY 1", // IRQ
    "   VEC_ENTRY 2", // FIQ
    "   VEC_ENTRY 3", // SError
    // 目前 EL，使用 SP_ELx  ← 核心自己出事會走這排
    "   VEC_ENTRY 4",
    "   VEC_ENTRY 5",
    "   VEC_ENTRY 6",
    "   VEC_ENTRY 7",
    // 來自較低 EL（AArch64）  ← 未來使用者程式出事會走這排
    "   VEC_ENTRY 8",
    "   VEC_ENTRY 9",
    "   VEC_ENTRY 10",
    "   VEC_ENTRY 11",
    // 來自較低 EL（AArch32）
    "   VEC_ENTRY 12",
    "   VEC_ENTRY 13",
    "   VEC_ENTRY 14",
    "   VEC_ENTRY 15",

    "__exception_common:",
    "   bl  rust_exception_handler",
    "1: wfe",
    "   b   1b",
);

extern "C" {
    static __vectors: u8;
}

/// 把向量表位址寫進 VBAR_EL1。在此之前發生例外會跳到 0x0，只能得到亂碼。
pub fn init() {
    let vbar = unsafe { &__vectors as *const u8 as u64 };
    unsafe {
        asm!("msr vbar_el1, {v}", "isb", v = in(reg) vbar, options(nostack));
    }
}

pub fn vbar() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, vbar_el1", out(reg) v, options(nomem, nostack)) };
    v
}

/// 16 個入口對應的名字。
const SOURCE: [&str; 16] = [
    "目前 EL / SP_EL0 · 同步",
    "目前 EL / SP_EL0 · IRQ",
    "目前 EL / SP_EL0 · FIQ",
    "目前 EL / SP_EL0 · SError",
    "目前 EL / SP_ELx · 同步",
    "目前 EL / SP_ELx · IRQ",
    "目前 EL / SP_ELx · FIQ",
    "目前 EL / SP_ELx · SError",
    "較低 EL (64-bit) · 同步",
    "較低 EL (64-bit) · IRQ",
    "較低 EL (64-bit) · FIQ",
    "較低 EL (64-bit) · SError",
    "較低 EL (32-bit) · 同步",
    "較低 EL (32-bit) · IRQ",
    "較低 EL (32-bit) · FIQ",
    "較低 EL (32-bit) · SError",
];

/// ESR_EL1 的 EC 欄位（bits 31:26）——例外的大分類。
fn describe_ec(ec: u64) -> &'static str {
    match ec {
        0x00 => "不明原因",
        0x0E => "非法的執行狀態",
        0x15 => "SVC 系統呼叫",
        0x18 => "被攔截的 MSR/MRS",
        0x20 => "指令擷取失敗（來自較低 EL）",
        0x21 => "指令擷取失敗（同一 EL）",
        0x22 => "PC 未對齊",
        0x24 => "資料存取失敗（來自較低 EL）",
        0x25 => "資料存取失敗（同一 EL）",
        0x26 => "SP 未對齊",
        0x30 | 0x31 => "硬體中斷點",
        0x3C => "BRK 除錯指令",
        _ => "（尚未收錄的 EC）",
    }
}

/// 資料/指令存取失敗時，ESR 低 6 位（DFSC/IFSC）說明「為什麼失敗」。
fn describe_fsc(fsc: u64) -> &'static str {
    match fsc {
        0x00..=0x03 => "位址大小錯誤",
        0x04 => "轉譯失敗 level 0（頁表根本沒建）",
        0x05 => "轉譯失敗 level 1",
        0x06 => "轉譯失敗 level 2",
        0x07 => "轉譯失敗 level 3",
        0x08..=0x0B => "存取旗標失敗",
        0x0C..=0x0F => "權限不足",
        0x10 => "外部匯流排錯誤",
        0x21 => "未對齊存取",
        _ => "（尚未收錄的 FSC）",
    }
}

#[no_mangle]
extern "C" fn rust_exception_handler(index: u64) -> ! {
    let (esr, far, elr, spsr): (u64, u64, u64, u64);
    unsafe {
        asm!("mrs {}, esr_el1",  out(reg) esr,  options(nomem, nostack));
        asm!("mrs {}, far_el1",  out(reg) far,  options(nomem, nostack));
        asm!("mrs {}, elr_el1",  out(reg) elr,  options(nomem, nostack));
        asm!("mrs {}, spsr_el1", out(reg) spsr, options(nomem, nostack));
    }

    let ec = (esr >> 26) & 0x3F;
    let fsc = esr & 0x3F;
    let source = SOURCE.get(index as usize).copied().unwrap_or("？");

    crate::println!();
    crate::println!("┌─ Lintsomax 攔到一個例外 ─────────────────────────");
    crate::println!("│ 來源     : [{:>2}] {}", index, source);
    crate::println!("│ 分類     : EC=0x{:02X}  {}", ec, describe_ec(ec));
    if ec == 0x24 || ec == 0x25 || ec == 0x20 || ec == 0x21 {
        crate::println!("│ 原因     : FSC=0x{:02X}  {}", fsc, describe_fsc(fsc));
        crate::println!("│ 出事位址 : 0x{:016X}   (FAR_EL1)", far);
    }
    crate::println!("│ 出事指令 : 0x{:016X}   (ELR_EL1)", elr);
    crate::println!("│ 原始 ESR : 0x{:016X}", esr);
    crate::println!("│ 原始 SPSR: 0x{:016X}", spsr);
    crate::println!("└──────────────────────────────────────────────────");
    crate::println!();
    crate::println!("在 Linux 上，這一刻整台機器已經 panic。");
    crate::println!("Lintsomax 的目標是：只有闖禍的那個元件倒下。");
    crate::println!("（M0 還沒有隔離，所以現在一樣只能停在這裡。）");

    crate::semihost::exit(0)
}

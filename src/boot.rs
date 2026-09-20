// SPDX-License-Identifier: GPL-3.0-or-later WITH Linux-syscall-note
//! 開機路徑：從 QEMU 交棒的那一刻到 `kernel_main`。

use core::arch::global_asm;

global_asm!(
    ".section .text.boot",
    ".global _start",
    "_start:",
    // 多核心開機時只讓 core 0 往下走，其餘原地睡
    "   mrs  x0, mpidr_el1",
    "   and  x0, x0, #0xff",
    "   cbnz x0, .Lpark",

    // 設定堆疊
    "   ldr  x0, =__stack_top",
    "   mov  sp, x0",

    // 清空 .bss（Rust 假設它是零）
    "   ldr  x0, =__bss_start",
    "   ldr  x1, =__bss_end",
    ".Lbss_loop:",
    "   cmp  x0, x1",
    "   b.hs .Lbss_done",
    "   str  xzr, [x0], #8",
    "   b    .Lbss_loop",
    ".Lbss_done:",

    "   bl   kernel_main",

    ".Lpark:",
    "   wfe",
    "   b    .Lpark",
);

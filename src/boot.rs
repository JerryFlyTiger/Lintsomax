// SPDX-License-Identifier: GPL-3.0-or-later WITH Linux-syscall-note
//! Boot path: from the moment QEMU hands over control to `kernel_main`.

use core::arch::global_asm;

global_asm!(
    ".section .text.boot",
    ".global _start",
    "_start:",
    // On a multi-core boot only core 0 proceeds; the rest park where they are.
    "   mrs  x0, mpidr_el1",
    "   and  x0, x0, #0xff",
    "   cbnz x0, .Lpark",
    // Set up the stack
    "   ldr  x0, =__stack_top",
    "   mov  sp, x0",
    // Zero .bss (Rust assumes it is zeroed)
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

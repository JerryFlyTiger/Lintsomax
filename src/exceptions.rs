// SPDX-License-Identifier: GPL-3.0-or-later WITH Linux-syscall-note
//! Exception vector table and fault reporting.
//!
//! This is what Lintsomax is about: a fault should not mean "the whole machine
//! goes down", it should mean "here is a report you can read". M0 delivers the
//! report; isolation is left to later milestones.

use core::arch::{asm, global_asm};

// 16 vector entries, 128 bytes each (.align 7); the whole table is 2048-aligned
// (.align 11). Every entry does just two things: put its own index in x0 and
// jump to the shared handler.
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
    // Current EL, using SP_EL0
    "   VEC_ENTRY 0", // Synchronous
    "   VEC_ENTRY 1", // IRQ
    "   VEC_ENTRY 2", // FIQ
    "   VEC_ENTRY 3", // SError
    // Current EL, using SP_ELx  <- a kernel fault lands in this row
    "   VEC_ENTRY 4",
    "   VEC_ENTRY 5",
    "   VEC_ENTRY 6",
    "   VEC_ENTRY 7",
    // From a lower EL (AArch64)  <- a future user program's fault lands here
    "   VEC_ENTRY 8",
    "   VEC_ENTRY 9",
    "   VEC_ENTRY 10",
    "   VEC_ENTRY 11",
    // From a lower EL (AArch32)
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

/// Write the vector table address into VBAR_EL1. Before this runs, an exception
/// jumps to 0x0 and all you get is garbage.
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

/// Names for the 16 vector entries.
const SOURCE: [&str; 16] = [
    "Current EL / SP_EL0 · Synchronous",
    "Current EL / SP_EL0 · IRQ",
    "Current EL / SP_EL0 · FIQ",
    "Current EL / SP_EL0 · SError",
    "Current EL / SP_ELx · Synchronous",
    "Current EL / SP_ELx · IRQ",
    "Current EL / SP_ELx · FIQ",
    "Current EL / SP_ELx · SError",
    "Lower EL (64-bit) · Synchronous",
    "Lower EL (64-bit) · IRQ",
    "Lower EL (64-bit) · FIQ",
    "Lower EL (64-bit) · SError",
    "Lower EL (32-bit) · Synchronous",
    "Lower EL (32-bit) · IRQ",
    "Lower EL (32-bit) · FIQ",
    "Lower EL (32-bit) · SError",
];

/// The EC field of ESR_EL1 (bits 31:26) - the broad class of the exception.
fn describe_ec(ec: u64) -> &'static str {
    match ec {
        0x00 => "unknown reason",
        0x0E => "illegal execution state",
        0x15 => "SVC system call",
        0x18 => "trapped MSR/MRS",
        0x20 => "instruction abort (from a lower EL)",
        0x21 => "instruction abort (same EL)",
        0x22 => "PC misaligned",
        0x24 => "data abort (from a lower EL)",
        0x25 => "data abort (same EL)",
        0x26 => "SP misaligned",
        0x30 | 0x31 => "hardware breakpoint",
        0x3C => "BRK debug instruction",
        _ => "(EC not catalogued yet)",
    }
}

/// On a data/instruction abort, the low 6 bits of ESR (DFSC/IFSC) say *why* it
/// failed.
fn describe_fsc(fsc: u64) -> &'static str {
    match fsc {
        0x00..=0x03 => "address size fault",
        0x04 => "translation fault level 0 (no page table at all)",
        0x05 => "translation fault level 1",
        0x06 => "translation fault level 2",
        0x07 => "translation fault level 3",
        0x08..=0x0B => "access flag fault",
        0x0C..=0x0F => "permission fault",
        0x10 => "external bus error",
        0x21 => "unaligned access",
        _ => "(FSC not catalogued yet)",
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
    let source = SOURCE.get(index as usize).copied().unwrap_or("?");

    crate::println!();
    crate::println!("┌─ Lintsomax caught an exception ─────────────────");
    crate::println!("│ Source      : [{:>2}] {}", index, source);
    crate::println!("│ Class       : EC=0x{:02X}  {}", ec, describe_ec(ec));
    if ec == 0x24 || ec == 0x25 || ec == 0x20 || ec == 0x21 {
        crate::println!("│ Reason      : FSC=0x{:02X}  {}", fsc, describe_fsc(fsc));
        crate::println!("│ Fault addr  : 0x{:016X}   (FAR_EL1)", far);
    }
    crate::println!("│ Fault instr : 0x{:016X}   (ELR_EL1)", elr);
    crate::println!("│ Raw ESR     : 0x{:016X}", esr);
    crate::println!("│ Raw SPSR    : 0x{:016X}", spsr);
    crate::println!("└──────────────────────────────────────────────────");
    crate::println!();
    crate::println!("On Linux, the whole machine would have panicked by now.");
    crate::println!("The goal of Lintsomax is that only the component at fault goes down.");
    crate::println!("(M0 has no isolation yet, so for now this is still where it stops.)");

    crate::semihost::exit(0)
}

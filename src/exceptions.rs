// SPDX-License-Identifier: GPL-3.0-or-later WITH Linux-syscall-note
//! Exception vector table and fault reporting.
//!
//! This is what Lintsomax is about: a fault should not mean "the whole machine
//! goes down", it should mean "here is a report you can read". M0 delivered the
//! report; M1.5 adds recovery: a single armed fault is reported and skipped
//! instead of halting the kernel.
//!
//! Limits of this design, not yet addressed:
//! (a) there is no nested-fault guard - a fault taken *inside* the handler
//!     itself (e.g. in the `println!` path) recurses through the same vector
//!     entry, pushing a new frame on top of the old one, until the stack is
//!     exhausted;
//! (b) the frame is only meaningfully saved for the current-EL/SP_ELx path in
//!     practice, since there is no lower EL yet for anything to fault from;
//! (c) resume means discard, not retry - `ELR_EL1 += 4` skips the faulting
//!     instruction; it does not fix anything and re-execute. A recovered load
//!     leaves its destination register holding whatever it held before, and a
//!     recovered store never happens. Any future caller that expects "fix the
//!     mapping, then retry" will get silently wrong results rather than a
//!     crash. (This is exactly why the `.rodata`/`.text` probes below read
//!     back unchanged - the write is discarded, not retried.)
//! (d) arming is a global single-shot flag, unrelated to any particular
//!     instruction - any unrelated exception occurring between
//!     `arm_recovery()` and the intended faulting instruction consumes the
//!     arming. The demo has no such window today because nothing between the
//!     two can fault, but the API is fragile by construction.
use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicBool, Ordering};

/// x0-x30 as saved by the vector entry, plus one slot of padding so the frame
/// stays 16-byte aligned.
#[repr(C)]
pub struct ExceptionFrame {
    pub x: [u64; 31],
    pub _pad: u64,
}

// These numbers are the same ones written in the `stp`/`ldp` offsets and the
// `sub sp, sp, #256` in the asm below. Changing one side without the other
// must fail the build, not silently corrupt the saved register state.
const _: () = assert!(core::mem::size_of::<ExceptionFrame>() == 256);
const _: () = assert!(core::mem::offset_of!(ExceptionFrame, x) == 0);
const _: () = assert!(core::mem::offset_of!(ExceptionFrame, _pad) == 248);

/// The handler's verdict, returned in x0 to the asm trampoline.
const HALT: u64 = 0;
const RESUME: u64 = 1;

// 16 vector entries, 128 bytes each (.align 7); the whole table is 2048-aligned
// (.align 11). Each entry saves x0/x1 (so the faulting x0 isn't clobbered by
// the index load), then loads its own index and jumps to the shared handler,
// which saves the rest of the registers, calls into Rust, and either resumes
// or halts based on the verdict.
global_asm!(
    ".macro VEC_ENTRY idx",
    "   .align 7",
    "   sub  sp, sp, #256",
    "   stp  x0, x1, [sp, #0]",
    "   mov  x0, #\\idx",
    "   b    __exception_common",
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
    "   stp  x2,  x3,  [sp, #16]",
    "   stp  x4,  x5,  [sp, #32]",
    "   stp  x6,  x7,  [sp, #48]",
    "   stp  x8,  x9,  [sp, #64]",
    "   stp  x10, x11, [sp, #80]",
    "   stp  x12, x13, [sp, #96]",
    "   stp  x14, x15, [sp, #112]",
    "   stp  x16, x17, [sp, #128]",
    "   stp  x18, x19, [sp, #144]",
    "   stp  x20, x21, [sp, #160]",
    "   stp  x22, x23, [sp, #176]",
    "   stp  x24, x25, [sp, #192]",
    "   stp  x26, x27, [sp, #208]",
    "   stp  x28, x29, [sp, #224]",
    "   str  x30,      [sp, #240]",
    "   mov  x1, sp",
    "   bl   rust_exception_handler",
    "   cbz  x0, .Lhalt",
    "   ldr  x30,      [sp, #240]",
    "   ldp  x28, x29, [sp, #224]",
    "   ldp  x26, x27, [sp, #208]",
    "   ldp  x24, x25, [sp, #192]",
    "   ldp  x22, x23, [sp, #176]",
    "   ldp  x20, x21, [sp, #160]",
    "   ldp  x18, x19, [sp, #144]",
    "   ldp  x16, x17, [sp, #128]",
    "   ldp  x14, x15, [sp, #112]",
    "   ldp  x12, x13, [sp, #96]",
    "   ldp  x10, x11, [sp, #80]",
    "   ldp  x8,  x9,  [sp, #64]",
    "   ldp  x6,  x7,  [sp, #48]",
    "   ldp  x4,  x5,  [sp, #32]",
    "   ldp  x2,  x3,  [sp, #16]",
    "   ldp  x0,  x1,  [sp, #0]",
    "   add  sp, sp, #256",
    "   eret",
    ".Lhalt:",
    "   bl   kernel_halt",
    // kernel_halt is `-> !` and never returns in practice (it terminates QEMU
    // via semihosting). This loop is only a backstop so that a kernel_halt
    // that somehow returns anyway cannot fall through into whatever code
    // happens to follow in .text.
    "   wfe",
    "   b    .Lhalt",
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

/// Set when the next synchronous data abort at the current EL should be
/// reported in one line, have its faulting instruction skipped, and let
/// execution continue - instead of the full report-and-halt path. Single-shot:
/// cleared the moment it is consumed, whether or not the next fault actually
/// matches.
static RECOVER_ARMED: AtomicBool = AtomicBool::new(false);

/// Arm a single recovery: the next synchronous data abort taken at the current
/// EL is reported in one line, its instruction is skipped, and execution
/// continues. Anything else still halts.
pub fn arm_recovery() {
    RECOVER_ARMED.store(true, Ordering::SeqCst);
}

/// Whether a recovery is currently armed. Exposed so the demo can show that
/// arming is single-shot: consumed by the first matching fault.
pub fn recovery_is_armed() -> bool {
    RECOVER_ARMED.load(Ordering::SeqCst)
}

/// The vector table has 16 entries arranged as four rows of four: for each of
/// the four exception sources (current EL/SP_EL0, current EL/SP_ELx, lower EL
/// AArch64, lower EL AArch32) the four entries are, in order, Synchronous /
/// IRQ / FIQ / SError. So an entry is the synchronous one for its row exactly
/// when its index is a multiple of 4.
///
/// This matters because `ESR_EL1` is only updated by synchronous exceptions.
/// It is not updated by IRQ or FIQ, so an IRQ taken while recovery is armed
/// could read a stale EC left over from an earlier synchronous fault and be
/// misjudged as recoverable.
fn is_synchronous_entry(index: u64) -> bool {
    index.is_multiple_of(4)
}

#[no_mangle]
extern "C" fn kernel_halt() -> ! {
    crate::semihost::exit(0)
}

#[no_mangle]
extern "C" fn rust_exception_handler(index: u64, frame: *mut ExceptionFrame) -> u64 {
    // M2 will use `frame` to report (and eventually restart) the faulting
    // task's register state; for now the frame is saved but not inspected.
    let _ = frame;

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

    let armed = RECOVER_ARMED.swap(false, Ordering::SeqCst);
    if armed && is_synchronous_entry(index) && (ec == 0x24 || ec == 0x25) {
        crate::println!(
            "  recovered: FSC=0x{:02X} {} at 0x{:016X}, skipping the instruction",
            fsc,
            describe_fsc(fsc),
            far
        );
        // A64 instructions are always 4 bytes wide, so advancing ELR_EL1 by 4
        // always lands exactly on the instruction after the faulting one.
        let elr_next = elr + 4;
        unsafe {
            asm!("msr elr_el1, {v}", "isb", v = in(reg) elr_next, options(nostack));
        }
        return RESUME;
    }

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
    if armed {
        crate::println!(
            "Recovery was armed, but this fault does not qualify (not a synchronous data \
             abort), so the kernel stops here."
        );
    } else {
        crate::println!("This fault was not armed for recovery, so the kernel stops here.");
    }

    HALT
}

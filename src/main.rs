// SPDX-License-Identifier: GPL-3.0-or-later WITH Linux-syscall-note
#![no_std]
#![no_main]

#[macro_use]
mod uart;

mod boot;
mod exceptions;
mod mm;
mod paging;
mod panic;
mod semihost;

use core::arch::asm;

extern "C" {
    static __text_start: u8;
}

/// A read-only probe used by the fault demo below: M1 maps `.rodata`
/// read-only, so writing here should take a permission fault.
#[used]
#[link_section = ".rodata"]
static RO_PROBE: u64 = 0x1234_5678_9ABC_DEF0;

fn current_el() -> u64 {
    let el: u64;
    unsafe { asm!("mrs {}, CurrentEL", out(reg) el, options(nomem, nostack)) };
    el >> 2
}

#[no_mangle]
extern "C" fn kernel_main() -> ! {
    uart::init();

    println!();
    println!("  ╭──────────────────────────────────────╮");
    println!("  │   L I N T S O M A X   ·   M1.5       │");
    println!("  │   aarch64 · Rust · QEMU virt         │");
    println!("  ╰──────────────────────────────────────╯");
    println!();
    println!("  [1/6] UART online                PL011 @ 0x0900_0000");
    println!("  [2/6] Current exception level    EL{}", current_el());

    exceptions::init();
    println!(
        "  [3/6] Vector table installed     VBAR_EL1 = 0x{:08X}",
        exceptions::vbar()
    );

    mm::init();
    let (used, total) = mm::stats();
    println!(
        "  [4/6] Frame allocator ready      {}/{} frames used",
        used, total
    );

    let ttbr0 = paging::init();
    println!(
        "  [5/6] Page tables built          TTBR0_EL1 = 0x{:016X}",
        ttbr0
    );

    paging::enable(ttbr0);
    println!("  [6/6] MMU enabled                SCTLR_EL1.M/C/I = 1");

    println!();
    println!("  This line still prints after the MMU is on, so the UART device mapping survived.");
    println!();
    let text_start = unsafe { &__text_start as *const u8 as u64 };
    println!("  Checking a few translations (identity mapping, so VA should equal PA):");
    for (name, va) in [
        ("UART       ", 0x0900_0000u64),
        ("kernel text", text_start),
        ("deep RAM   ", 0x4100_0000u64),
    ] {
        match paging::translate(va) {
            Some(pa) => println!("    {} 0x{:010X} → 0x{:010X}", name, va, pa),
            None => println!("    {} 0x{:010X} → translation failed", name, va),
        }
    }

    let (used, total) = mm::stats();
    println!();
    println!(
        "  Frame allocator now: {}/{} frames used (the page tables count too)",
        used, total
    );

    println!();
    println!("  Fault isolation demo: four faults, the first three are recovered");
    println!();

    // Fault 1/4 - translation fault on an unmapped address.
    println!("  [1/4] Reading an unmapped address - expect a recovered translation fault.");
    exceptions::arm_recovery();
    let bad = 0x0000_0000_8000_0000u64 as *const u64;
    unsafe { core::ptr::read_volatile(bad) };
    println!("  [1/4] Still running after the fault.");
    if exceptions::recovery_is_armed() {
        println!("  [1/4] Arming flag after the fault: still armed (unexpected!).");
    } else {
        println!("  [1/4] Arming flag after the fault: consumed (single-shot).");
    }

    // Fault 2/4 - write to .rodata, which M1 maps read-only.
    // Use `addr_of!` rather than `&RO_PROBE as *const u64`: the latter would
    // create a shared reference to a `static` we are about to (attempt to)
    // write through, which is its own flavor of undefined behavior regardless
    // of what the MMU does.
    let probe_ptr = core::ptr::addr_of!(RO_PROBE);
    let before = unsafe { core::ptr::read_volatile(probe_ptr) };
    println!(
        "  [2/4] .rodata probe at 0x{:016X} = 0x{:016X} (mapped read-only by M1).",
        probe_ptr as u64, before
    );
    println!("  [2/4] Attempting a write to it - expect a recovered permission fault.");
    exceptions::arm_recovery();
    unsafe { core::ptr::write_volatile(probe_ptr as *mut u64, 0xDEAD_BEEF) };
    let after = unsafe { core::ptr::read_volatile(probe_ptr) };
    if after == before {
        println!(
            "  [2/4] Probe still reads 0x{:016X} - unchanged, the read-only mapping held.",
            after
        );
    } else {
        println!(
            "  [2/4] LOUD WARNING: probe now reads 0x{:016X} - the read-only mapping did NOT hold!",
            after
        );
    }

    // Fault 3/4 - write to .text, which M1 maps read-only (and, unlike
    // .rodata, must not be writable at all - PLAN.md names this exact gap).
    // Use `addr_of!` for the same reason as the .rodata probe above: forming
    // a shared reference to memory we are about to write through would be UB
    // regardless of what the MMU does, and clippy's `invalid_reference_casting`
    // (deny-by-default) would flag it anyway.
    let text_ptr = core::ptr::addr_of!(__text_start);
    let text_before = unsafe { core::ptr::read_volatile(text_ptr) };
    println!(
        "  [3/4] .text probe at 0x{:016X} = 0x{:02X} (mapped read-only by M1).",
        text_ptr as u64, text_before
    );
    println!("  [3/4] Attempting a write to it - expect a recovered permission fault.");
    exceptions::arm_recovery();
    unsafe { core::ptr::write_volatile(text_ptr as *mut u8, 0xFF) };
    let text_after = unsafe { core::ptr::read_volatile(text_ptr) };
    if text_after == text_before {
        println!(
            "  [3/4] Probe still reads 0x{:02X} - unchanged, the read-only mapping held.",
            text_after
        );
    } else {
        println!(
            "  [3/4] LOUD WARNING: probe now reads 0x{:02X} - the read-only mapping did NOT hold!",
            text_after
        );
    }

    // Fault 4/4 - unrecoverable, but not because recovery is unarmed: this
    // one is armed on purpose, to show that the EC filter (not just the
    // arming flag) is what stands between a fault and recovery. An SVC is a
    // synchronous exception to EL1 (EC=0x15), so it passes the "is this
    // synchronous" check but fails the "is this a data abort" check, and the
    // kernel should still halt.
    println!();
    println!("  [4/4] Arming recovery, then executing SVC #0 - recovery is armed on purpose,");
    println!("  [4/4] but an SVC is not a data abort, so the EC filter should refuse it and");
    println!("  [4/4] the kernel should still halt.");
    exceptions::arm_recovery();
    unsafe { asm!("svc #0") };

    // Reaching this point means the fourth, deliberately unrecoverable fault
    // did not halt the kernel, which is a bug.
    panic!("the fourth fault escaped the vector table instead of halting");
}

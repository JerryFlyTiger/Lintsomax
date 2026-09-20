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
    println!("  │   L I N T S O M A X   ·   M1         │");
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
    println!("  The kernel is alive. Next it deliberately reads an unmapped address");
    println!("  to see whether the vector table catches it - this should be a");
    println!("  translation fault, not an address size fault.");

    let bad = 0x0000_0000_8000_0000u64 as *const u64;
    unsafe { core::ptr::read_volatile(bad) };

    // Reaching this point means the vector table did not catch it, which is a bug.
    panic!("exception was not trapped - the vector table is not in effect");
}

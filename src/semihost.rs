// SPDX-License-Identifier: GPL-3.0-or-later WITH Linux-syscall-note
//! ARM semihosting: lets the kernel terminate QEMU on its own, so that
//! `cargo run` exits cleanly instead of leaving the terminal stuck.

use core::arch::asm;

const SYS_EXIT: u64 = 0x18;
const APP_EXIT: u64 = 0x2_0026; // ADP_Stopped_ApplicationExit

pub fn exit(code: u32) -> ! {
    let block = [APP_EXIT, code as u64];
    unsafe {
        asm!(
            "hlt #0xF000",
            in("x0") SYS_EXIT,
            in("x1") block.as_ptr(),
            options(noreturn, nostack),
        );
    }
}

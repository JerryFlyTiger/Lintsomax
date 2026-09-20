//! ARM 半主機（semihosting）：讓核心可以主動結束 QEMU，
//! `cargo run` 才會正常收工，而不是把終端機卡住。

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

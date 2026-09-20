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
    println!("  [1/6] UART 上線                  PL011 @ 0x0900_0000");
    println!("  [2/6] 目前特權等級               EL{}", current_el());

    exceptions::init();
    println!("  [3/6] 例外向量表已安裝           VBAR_EL1 = 0x{:08X}", exceptions::vbar());

    mm::init();
    let (used, total) = mm::stats();
    println!("  [4/6] 實體框配置器初始化         已用 {}/{} 框", used, total);

    let ttbr0 = paging::init();
    println!("  [5/6] 頁表建好                   TTBR0_EL1 = 0x{:016X}", ttbr0);

    paging::enable(ttbr0);
    println!("  [6/6] MMU 已開啟                 SCTLR_EL1.M/C/I = 1");

    println!();
    println!("  MMU 開啟後這行字還印得出來，代表 UART 的 device 映射沒壞。");
    println!();
    let text_start = unsafe { &__text_start as *const u8 as u64 };
    println!("  驗證幾個位址的轉譯（恆等映射，VA 應該等於 PA）：");
    for (name, va) in [
        ("UART       ", 0x0900_0000u64),
        ("kernel text", text_start),
        ("RAM 深處   ", 0x4100_0000u64),
    ] {
        match paging::translate(va) {
            Some(pa) => println!("    {} 0x{:010X} → 0x{:010X}", name, va, pa),
            None => println!("    {} 0x{:010X} → 轉譯失敗", name, va),
        }
    }

    let (used, total) = mm::stats();
    println!();
    println!("  框配置器目前狀態：已用 {}/{} 框（頁表本身也算在內）", used, total);

    println!();
    println!("  核心活著。接下來故意讀一個沒有映射的位址，");
    println!("  看看向量表接不接得住——這次應該是轉譯失敗，不是位址大小錯誤。");

    let bad = 0x0000_0000_8000_0000u64 as *const u64;
    unsafe { core::ptr::read_volatile(bad) };

    // 走到這裡代表向量表沒接住，那就是 bug
    panic!("例外沒有被攔截——向量表沒生效");
}

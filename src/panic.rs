// SPDX-License-Identifier: GPL-3.0-or-later WITH Linux-syscall-note
//! Rust panic 的落地點。目前直接印出來然後收工。

use core::panic::PanicInfo;

#[panic_handler]
fn on_panic(info: &PanicInfo) -> ! {
    crate::println!();
    crate::println!("┌─ Lintsomax panic ───────────────────────────────");
    if let Some(loc) = info.location() {
        crate::println!("│ 位置 : {}:{}:{}", loc.file(), loc.line(), loc.column());
    }
    crate::println!("│ 訊息 : {}", info.message());
    crate::println!("└──────────────────────────────────────────────────");
    crate::semihost::exit(1)
}

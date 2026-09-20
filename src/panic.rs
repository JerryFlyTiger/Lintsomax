// SPDX-License-Identifier: GPL-3.0-or-later WITH Linux-syscall-note
//! Landing site for Rust panics. For now it prints the report and shuts down.

use core::panic::PanicInfo;

#[panic_handler]
fn on_panic(info: &PanicInfo) -> ! {
    crate::println!();
    crate::println!("┌─ Lintsomax panic ───────────────────────────────");
    if let Some(loc) = info.location() {
        crate::println!("│ Location: {}:{}:{}", loc.file(), loc.line(), loc.column());
    }
    crate::println!("│ Message : {}", info.message());
    crate::println!("└──────────────────────────────────────────────────");
    crate::semihost::exit(1)
}

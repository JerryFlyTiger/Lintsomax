//! PL011 UART —— QEMU virt 把它映在 0x0900_0000。

use core::fmt::{self, Write};

const BASE: usize = 0x0900_0000;

const DR: usize = 0x00; // Data
const FR: usize = 0x18; // Flag
const IBRD: usize = 0x24; // Integer baud rate divisor
const FBRD: usize = 0x28; // Fractional baud rate divisor
const LCRH: usize = 0x2C; // Line control
const CR: usize = 0x30; // Control

const FR_TXFF: u32 = 1 << 5; // 傳送 FIFO 滿了

const fn reg(off: usize) -> *mut u32 {
    (BASE + off) as *mut u32
}

pub fn init() {
    unsafe {
        core::ptr::write_volatile(reg(CR), 0); // 先關掉再設定
        core::ptr::write_volatile(reg(IBRD), 13); // 24 MHz / (16 × 115200)
        core::ptr::write_volatile(reg(FBRD), 1);
        core::ptr::write_volatile(reg(LCRH), (0b11 << 5) | (1 << 4)); // 8-N-1 + FIFO
        core::ptr::write_volatile(reg(CR), (1 << 0) | (1 << 8) | (1 << 9)); // 啟用 + TX + RX
    }
}

fn put(byte: u8) {
    unsafe {
        while core::ptr::read_volatile(reg(FR)) & FR_TXFF != 0 {}
        core::ptr::write_volatile(reg(DR), byte as u32);
    }
}

pub struct Uart;

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                put(b'\r'); // 終端機要 CRLF
            }
            put(b);
        }
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    let _ = Uart.write_fmt(args);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::uart::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    ()               => ($crate::print!("\n"));
    ($($arg:tt)*)    => ($crate::print!("{}\n", format_args!($($arg)*)));
}

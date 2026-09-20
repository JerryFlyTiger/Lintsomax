/* QEMU virt 的 RAM 從 0x4000_0000 開始；0x4010_0000 留一段給未來的 DTB */
ENTRY(_start)

SECTIONS
{
  . = 0x40100000;

  . = ALIGN(4096);
  __text_start = .;
  .text : ALIGN(8) {
    KEEP(*(.text.boot))     /* _start 必須排第一 */
    *(.text .text.*)
  }
  __text_end = .;

  . = ALIGN(4096);
  __rodata_start = .;
  .rodata : ALIGN(8) { *(.rodata .rodata.*) }
  __rodata_end = .;

  . = ALIGN(4096);
  __data_start = .;
  .data   : ALIGN(8) { *(.data .data.*) }

  .bss : ALIGN(16) {
    __bss_start = .;
    *(.bss .bss.*)
    *(COMMON)
    . = ALIGN(16);
    __bss_end = .;
  }

  . = ALIGN(16);
  . = . + 0x10000;          /* 64 KiB 開機堆疊 */
  __stack_top = .;

  . = ALIGN(4096);
  __kernel_end = .;

  /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}

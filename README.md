# Lintsomax

A kernel written to fix two structural pain points in Linux:

1. **No fault isolation** -- a bug in one driver can panic the entire machine
2. **Memory unsafety** -- the kernel is written in C

Secondary goals: multi-tenant isolation, a capability-based permission model.

## Decisions made

| Item | Decision | Rationale |
|---|---|---|
| Architecture | Microkernel (capability-based) | Monolithic is the pain point itself; a unikernel has no internal isolation; a hybrid design compromises from the start |
| Language | Rust `no_std` | Addresses pain point 2. `unsafe` is confined to a small number of modules |
| Target platform | aarch64 . QEMU `virt` | Dev machine is Apple Silicon, so HVF can accelerate it; aarch64 boot and memory model are clean |
| Hardware scope | virtio only, no bare-metal drivers | Drivers make up 60-70% of Linux's source code; skipping them drops the cost by an order of magnitude |
| Memory model order | aarch64 (weak ordering) first, x86 (strong ordering) later | Weak-to-strong is safe; the reverse order hides race conditions that won't surface |

## Running it

```bash
cargo run            # boot, run the demo, shut down on its own
cargo run -- trace   # add QEMU exception tracing, written to qemu-int.log
```

## Debugging

```bash
# Terminal A
cargo run -- debug
# Terminal B
./debug.sh
```

`.gdbinit` automatically loads the Rust pretty-printer and connects to `:1234`.

**Watch out for `opt-level`**: the `dev` profile must stay at `opt-level = 0`.
In practice, `opt-level = 1` turns every local variable into `<optimized out>`,
and breakpoints end up landing inside inlined `core` source code.

## Status: M1 complete

- [x] **M0** aarch64 boot, halts at EL1, only core 0 runs on multi-core
- [x] **M0** PL011 UART + `println!`
- [x] **M0** exception vector table (16 entries) + ESR/FAR decode report
- [x] **M0** Rust panic handler, shutdown via semihosting
- [x] **M1** physical frame allocator (bump)
- [x] **M1** four-level page tables, identity mapping, MMU enabled
- [x] **M1** `.text` read-only and executable, data regions non-executable
- [ ] **M1.5** making the vector table recoverable <- next step
- [ ] **M2** capability table, higher-half kernel (TTBR1) migration
- [ ] **M3** IPC
- [ ] **M4** first isolated virtio driver <- first real demo of fault isolation

## Source layout

```
src/boot.rs        _start: park cores, set up stack, clear .bss
src/uart.rs        PL011 driver + print!/println!
src/exceptions.rs  vector table, ESR/FAR decoding        <- where the project's core topic lives
src/mm.rs          physical memory frame allocator (bump)
src/paging.rs      four-level page tables, identity mapping, MMU on/off
src/panic.rs       Rust panic landing
src/semihost.rs    lets the kernel actively terminate QEMU
linker.ld          memory layout (loaded at 0x4010_0000)
```

## Known issues (found by cold review, not fixed yet)

See "Review log" in `PLAN.md`. The most important one: **the current
self-verification doesn't reliably detect incorrectly written page table
permission bits** -- in practice, swapping `AP_RW`/`AP_RO` causes the kernel
to die silently when the MMU is enabled, with no diagnostic message at all.
M1.5's recoverable exceptions exist specifically to close this gap.

## License

**GPL-3.0-or-later, plus a syscall exception.**

| File | Contents |
|---|---|
| `LICENSE` | Full text of GNU GPL v3 (unmodified, so GitHub recognizes it correctly) |
| `LICENSE-EXCEPTION` | Syscall exception (English only) |

**Modifications to the kernel source itself**: fully covered by GPL-3.0-or-later.
You can modify it and sell it, but any changes must be released under the same terms.

**User-space programs running on Lintsomax are unaffected** -- whether they interact
through system calls, IPC messages, capability invocations, or shared memory created
through those interfaces. These are not considered derivative works, and their authors
may choose any license they like.

This exception exists because Lintsomax is a microkernel: **drivers, file systems,
and network stacks are deliberately designed as separate user-space programs**.
Without this exception, the system's normal architecture would make every such
component a derivative work of the kernel, which is not the intent of this project.
The approach follows Linux's `COPYING`.

Every source file starts with an SPDX header:

```
// SPDX-License-Identifier: GPL-3.0-or-later WITH Linux-syscall-note
```

`Linux-syscall-note` is an SPDX-registered exception identifier, originally
defined for Linux's use of GPL-2.0. This line is only a machine-readable
approximation for tooling -- **the exception terms that actually apply are
the text in `LICENSE-EXCEPTION`**.

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

## Status: M1.5 complete

- [x] **M0** aarch64 boot, halts at EL1, only core 0 runs on multi-core
- [x] **M0** PL011 UART + `println!`
- [x] **M0** exception vector table (16 entries) + ESR/FAR decode report
- [x] **M0** Rust panic handler, shutdown via semihosting
- [x] **M1** physical frame allocator (bump)
- [x] **M1** four-level page tables, identity mapping, MMU enabled
- [x] **M1** `.text` read-only and executable, data regions non-executable
- [x] **M1.5** register save/restore frame, recoverable faults, `.rodata` and `.text` permission probes
- [ ] **M2** capability table, higher-half kernel (TTBR1) migration <- next step
- [ ] **M3** IPC
- [ ] **M4** first isolated virtio driver <- first real demo of fault isolation

## Source layout

```
src/boot.rs        _start: park cores, set up stack, clear .bss
src/uart.rs        PL011 driver + print!/println!
src/exceptions.rs  vector table, register frame, fault recovery, ESR/FAR decoding  <- where the project's core topic lives
src/mm.rs          physical memory frame allocator (bump)
src/paging.rs      four-level page tables, identity mapping, MMU on/off
src/panic.rs       Rust panic landing
src/semihost.rs    lets the kernel actively terminate QEMU
linker.ld          memory layout (loaded at 0x4010_0000)
```

## Fault isolation demo

`cargo run` takes four deliberate faults and survives the first three:

```
  [1/4] Reading an unmapped address - expect a recovered translation fault.
  recovered: FSC=0x05 translation fault level 1 at 0x0000000080000000, skipping the instruction
  [1/4] Still running after the fault.
  [1/4] Arming flag after the fault: consumed (single-shot).
  [2/4] .rodata probe at 0x0000000040107898 = 0x123456789ABCDEF0 (mapped read-only by M1).
  recovered: FSC=0x0F permission fault at 0x0000000040107898, skipping the instruction
  [2/4] Probe still reads 0x123456789ABCDEF0 - unchanged, the read-only mapping held.
  [3/4] .text probe at 0x0000000040100000 = 0xA0 (mapped read-only by M1).
  recovered: FSC=0x0F permission fault at 0x0000000040100000, skipping the instruction
  [3/4] Probe still reads 0xA0 - unchanged, the read-only mapping held.
  [4/4] Arming recovery, then executing SVC #0 ... the kernel should still halt.
```

The last fault is armed for recovery on purpose and still halts, because an
SVC is not a data abort. That is the EC filter refusing it, not the arming
flag.

Faults 2 and 3 are the point of the milestone. M1 could map `.text` or
`.rodata` with the wrong permission bits and nothing would notice: with an
identity mapping, wrong permissions only show up when something actually
tries the access. Nothing wrote to read-only memory, so nothing ever checked.
Swapping `AP_RW`/`AP_RO` back then produced a silent death with no diagnostic
at all. Now there is a test that writes to read-only memory and expects to be
refused, and a mutation of either mapping prints a loud warning instead.

Recovery means the faulting instruction is **discarded, not retried**:
`ELR_EL1 += 4` skips it. A recovered load leaves its destination register
holding whatever it held before, and a recovered store never happens. That is
why both probes read back unchanged. See the limits list at the top of
`src/exceptions.rs`.

## Known issues

See "Review log" in `PLAN.md`. The largest remaining gap: the recovery path
only accepts faults taken through a **synchronous** vector entry, because
`ESR_EL1` is not updated by IRQ or FIQ and a stale `EC` could make an
interrupt look recoverable. That filter is currently untestable -- removing it
changes no output, since nothing can raise an interrupt yet. It gets a real
test in M3.

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

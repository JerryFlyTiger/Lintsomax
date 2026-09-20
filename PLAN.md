# Lintsomax progress and handoff

Last updated: 2026-09-20

## Goals (settled, not up for discussion)

- **Primary**: fault isolation (a bug in one driver shouldn't take down the whole machine) + memory safety
- **Secondary**: multi-tenant isolation, capability-based permission model
- Positioning: **a hobby project**, not a Linux replacement. Success is defined as "solving the pain point better than Linux in a narrow scenario, with runnable evidence"

## Settled technical decisions (rationale in README.md)

| Item | Decision |
|---|---|
| Architecture | Microkernel, capability-based |
| Language | Rust `no_std`, nightly |
| Platform | aarch64 . QEMU `virt` . HVF for measurement, TCG for debugging on Apple Silicon |
| Hardware scope | virtio only, no bare-metal drivers |
| Debugger | `aarch64-elf-gdb` (not lldb). Reason: system register + Rust enum support |
| `dev` profile | **`opt-level = 0` must not change**. In practice, 1 turns every variable into `<optimized out>` |

## Milestones

- [x] **M0** boot . EL1 . PL011 UART . `println!` . exception vector table . ESR/FAR decoding . shutdown via semihosting
- [x] **M1** physical frame allocator . four-level page tables . identity mapping . MMU enabled . translation faults decoded correctly
- [x] **M1.5** register save/restore frame . recoverable faults . `.rodata` and `.text` permission probes . EC filter
- [ ] **M2** capability table . higher-half kernel (TTBR1) migration <- **next step, handoff point is here**
- [ ] **M3** IPC
- [ ] **M4** first isolated virtio driver <- first real demo of fault isolation

## M1.5 as built

`__exception_common` now saves x0-x30 into a 256-byte `ExceptionFrame` and
branches on the handler's verdict: `cbz x0, .Lhalt` halts, anything else
restores all registers and `eret`s. Each vector entry saves x0/x1 *before*
loading its index - the old code clobbered x0 with the index first, which made
recovery impossible by construction.

Recovery is a single-shot global flag, `arm_recovery()`. A fault is recovered
only if it is armed, taken through a synchronous vector entry, and has
`EC` 0x24/0x25 (data abort). Recovery means `ELR_EL1 += 4`: the faulting
instruction is **discarded, not retried**.

The demo takes four faults: unmapped read (recovered), `.rodata` write
(recovered permission fault), `.text` write (recovered permission fault),
then an armed `svc #0` that the EC filter refuses, which halts.

## Handoff point: M2 capability table and higher-half kernel

Not yet specified. Two items already queued for M2 by earlier review rounds:

1. **`paging.rs`'s `table_ref()` aliasing UB** (item 5 in the first review
   round below). M2 builds per-task address spaces and has to refactor this
   anyway; switch it to raw pointers then.
2. **The `EPD1` safeguard is a black box** - nothing accesses an upper-half VA,
   so reverting that fix changes no output. M2's higher-half migration should
   add a test that deliberately touches a high VA and expects a level-0
   translation fault.

M1.5 also leaves `frame` saved but never dereferenced (`let _ = frame;`). M2
is where it starts being read, to report the faulting task's register state.

## Review log

### First cold review round (M0 + M1, 830 lines)

**Passed**: page table descriptor bit encoding (AttrIndx/AP/SH/AF/PXN/UXN
positions), TCR_EL1 fields (including the TG0=0b00 vs TG1=0b10 encoding
mismatch trap), MMU on/off barrier ordering, PL011 registers, vector table
alignment, ESR decode table, `.bss` zeroing boundaries, semihosting conventions --
all correct after a bit-by-bit check.

**Fixed**:

| # | Issue | Fix |
|---|---|---|
| 1 | `cargo clippy -- -D warnings` build failure (2 lints) | `paging.rs` switched to `is_multiple_of`; removed the no-op `(0b00 << 14)` expression, replaced with a comment |
| 3 | `TCR_EL1.EPD1` unset, table walks still enabled while TTBR1 uninitialized | Added `1 << 23` |
| 4 | release build strips `debug_assert`, so alignment assumption violations happen silently | Added `debug-assertions = true` to the release profile in `Cargo.toml` |
| 6 | README stuck at M0 | Updated |

**Not fixed, with reasons**:

| # | Issue | Why not fixed |
|---|---|---|
| 2 | Self-verification doesn't reliably detect incorrectly written permission bits | **This is exactly what M1.5 is fixing.** See the mutation test results below |
| 5 | `paging.rs`'s `table_ref()` produces a new `&'static mut` for the same PA every call, which is aliasing UB at the Rust abstract machine level | No observable effect under single-core + `opt-level=0`; can't be verified with mutation testing (Miri doesn't run on this target). **M2's per-task address spaces will require refactoring this anyway, so it will be switched to a raw pointer then** |
| 7 | `VBAR_EL1` unset and `DAIF` unmasked when parking non-core-0 | `run.sh` doesn't pass `-smp`, so QEMU only starts 1 vCPU and this path never executes. Multi-core is a later milestone |
| 8 | Device memory uses `SH=0b00` instead of Outer-shareable | Low confidence. Device-nGnRE ordering semantics don't rely on the SH field; no effect with a single core |

### Mutation testing (run by the main conversation, not the reviewer)

- **A (clippy)**: confirmed failure, not a guess. Fixed.
- **B (swap `AP_RW` / `AP_RO`)**: **the reviewer predicted "output completely
  unchanged", which turned out to be wrong.**
  The actual result is the kernel dies silently before printing
  `[6/6] MMU enabled`.
  Note that `device_rw()` and `normal_rw()` share the same `AP_RW` constant,
  so this mutation makes both the stack **and** UART MMIO read-only at the same
  time. There's no way to tell from the symptom alone which write fails first,
  but the flow suggests the stack fails first -- under `opt-level=0`, every
  `println!` call has a function prologue that writes to the stack at each
  level, while `put()` reads `FR` (unaffected) before writing `DR`.
  Either way, the causal chain holds: read-only stack -> the exception
  handler's own function prologue also faults -> recursive fault ->
  silent lockup.
  Revised conclusion: **it is detectable, but only manifests as "nothing gets
  printed and it just dies", which is the hardest failure mode to diagnose**;
  and it only catches "a data region mistakenly marked read-only", not
  "`.text` mistakenly marked writable" -- nothing ever writes to `.text`.
  This is exactly the gap M1.5 is meant to close: a test that **actively
  writes to `.rodata` and expects to get a permission fault**.

### Second cold review round (fix round, tail diff)

**Passed, no bugs found.** Checked item by item: `is_multiple_of` is
semantically equivalent; after removing `(0b00 << 14)`, the `tcr` value is
bit-for-bit identical and nothing else fills bits[15:14]; **bit 23 is indeed
`EPD1`** (`A1` is bit 22, `IRGN1` starts at bit 24, no overlap), and setting it
to 1 means disabling TTBR1 table walks.

Concerns ruled out: `debug-assertions = true` **does not** also enable
`overflow-checks` (confirmed in practice), and since this project doesn't use
`-Zbuild-std`, `core` is rustup's precompiled rlib, so this setting only
affects the `lintsomax` crate itself and won't bloat the panic path.

**Two new coverage gaps found (to-do, not defects)**:

- **The `EPD1` safeguard is currently a black box.** No code accesses the
  upper-half VA, so reverting this fix would leave `cargo run`'s output
  bit-for-bit identical. -> **When M2's higher-half migration happens, add
  a test that deliberately accesses a high VA and expects a level-0
  translation fault.**
- **`debug-assertions` similarly misses the test paths.** The only
  `debug_assert_eq!` in `mm.rs:22` always holds on every current path.
  -> Low priority, handle it alongside the next linker script change.

### M1.5 cold review (first round)

**No correctness bug found in the save/restore assembly.** The reviewer
disassembled `__exception_common` and checked all 31 register offsets, the
256-byte frame size, SP alignment, and that the handler's own stack frame
cannot overlap the saved registers. It also confirmed the SPSR/NZCV reasoning
(PSTATE is restored from the `SPSR_EL1` snapshot, so the handler cannot corrupt
the resumed code's condition flags) and that `ELR_EL1 += 4` is sound on A64:
exceptions are precise and instructions are a fixed 4 bytes, so unlike x86
there is no case where the faulting instruction is not the one at `ELR`.

It also confirmed the `.rodata` probe passes **for the right reason**:
`RO_PROBE` is at `0x40107898`, inside `__rodata_start=0x40107000 ..
__rodata_end=0x401095cb`, and `paging.rs` maps exactly that range
`normal_ro_noexec()` with no other mapping loop overlapping it.

What it found instead were defences that were real but unverifiable. All fixed
in the same milestone:

| # | Finding | Fix |
|---|---|---|
| 1 | `HALT` was dead code - the halt path called `semihost::exit` directly, so `cbz x0, .Lhalt` had never once fired | Handler returns `HALT`; `.Lhalt` calls a new `kernel_halt` |
| 2 | `ExceptionFrame`'s layout matched the asm's `256`/`240` only by human eyeballing; `frame` is never dereferenced, so breaking the struct compiled clean | `const _: () = assert!(size_of/offset_of ...)` |
| 3 | Every demo fault was a data abort, so deleting the EC filter changed nothing | Fault 4/4 is an armed `svc #0` (EC=0x15) the filter must refuse |
| 4 | `.text`-mapped-writable was still untested - the gap named below in the M1 mutation notes | Fault 3/4 writes to `__text_start` |
| 5 | The handler trusted `ESR_EL1` for all 16 vector entries; IRQ/FIQ do not update it, so a stale `EC` could make an interrupt look recoverable and skip 4 bytes of good code | Recovery also requires `is_synchronous_entry(index)` |
| 6 | Resume means discard, not retry - undocumented | Module docs, limit (c) |
| 7 | Arming is a global flag; any unrelated fault between `arm_recovery()` and the target instruction consumes it | Documented only, limit (d). No window exists today |

Finding 5 is the one worth remembering: the bug was unreachable only because
nothing has unmasked `DAIF` yet. M3 enables interrupts.

### M1.5 mutation battery (main conversation, not the reviewer)

Eight one-line mutations, each rebuilt and run:

| Mutation | Result |
|---|---|
| `elr + 4` -> `elr` | Fault 1 repeats, halts with the full report; `[1/4] Still running` never prints |
| `swap(false, ..)` -> `load(..)` | Prints `still armed (unexpected!)` |
| `cbz x0, .Lhalt` -> `cbnz` | Halts immediately after fault 1's recovered line, exit 0 |
| `.rodata` `AP_RO` -> `AP_RW` | `[2/4] LOUD WARNING`; `[3/4]` still holds |
| `.text` `AP_RO` -> `AP_RW` | `[3/4] LOUD WARNING`; `[2/4]` still holds |
| EC filter removed | The SVC is recovered, execution reaches the `panic!` backstop |
| `ExceptionFrame._pad` removed | Build fails on the const assert |
| `is_synchronous_entry` -> `true` | **Output byte-identical - still a black box** |

Seven of the eight defences are observable. The synchronous-entry filter is
not, and cannot be until something can raise an IRQ. **Queued for M3**: with
interrupts enabled, arm recovery, take an IRQ while `ESR_EL1` still holds a
stale data-abort `EC`, and confirm the kernel does not skip the interrupted
instruction.

### M1.5 cold review (fix round) and the second mutation battery

**No new correctness bug.** The reviewer disassembled the new paths and
confirmed, rather than inferred: `rust_exception_handler` has a single
epilogue and only two instructions ever write the return slot (`mov x8, xzr`
and `mov w8, #1`), so the value `cbz` inspects can only be 0 or 1; both
verdict paths restore `sp` to the bottom of the 256-byte frame per AAPCS64.
It also confirmed the `.text` probe reads a real instruction byte: `0x40100000`
holds `0xA0`, the low byte of `_start`'s first instruction `mrs x0, MPIDR_EL1`
(`d5 38 00 a0`, little-endian).

Two things it clarified that are worth keeping:

- **`bl kernel_halt` rather than `b` is correct, for a counter-intuitive
  reason.** `kernel_halt` is `-> !`, so the call never returns; but if it ever
  did, `bl` leaves x30 pointing at the `wfe` backstop. With `b`, x30 would
  still hold the stale value from the `bl rust_exception_handler` call and
  falling out would jump somewhere meaningless.
- **The asm `wfe` backstop is nearly unreachable**, because LLVM emits its own
  `brk #0x1` after the `hlt #0xF000` in `semihost::exit` (its standard guard
  for noreturn inline asm). A returning `hlt` hits that `brk` first and
  recurses into the same vector table - which is limit (a), the missing
  nested-fault guard, not a new problem.

Four further mutations it designed, all run:

| Mutation | Result |
|---|---|
| Delete `bl kernel_halt` | QEMU never exits; `timeout` kills the run (rc=124) |
| Halt message `if armed` -> `if false` | Prints "not armed for recovery" instead of the armed-but-unqualified message |
| `index.is_multiple_of(4)` -> `(index + 1).is_multiple_of(4)` | Fault 1 is refused and halts; `[1/4] Still running` never prints |
| `ExceptionFrame` fields reordered | Build fails on `offset_of!(.., x) == 0` |

The first pins `kernel_halt` itself, which the `cbz`/`cbnz` mutation only
tested the branch direction of. The third pins the modulus constant, not just
that the check is wired up.

**Still a black box, with no mutation possible:** that `frame.x[i]` corresponds
to register `xi`. The const assertions pin the struct's *layout*, but the
register mapping is a convention between the asm and future Rust code, and
nothing reads `frame.x[i]` yet. It becomes testable in M2.

## Review status

- M0 + M1 are committed (`0f6ce20`), the license as `26ef066`, and the
  English-only translation of the whole tree as `638e9a6`.
- All cold review rounds above are complete.
- **No commits until every change has been cold-reviewed.**

## Conventions

- **Everything in this project is written in English** - source comments, kernel
  output strings, docs, commit messages. No other language anywhere in the tree.

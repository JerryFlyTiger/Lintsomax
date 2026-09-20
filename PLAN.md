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
- [ ] **M1.5** making the vector table recoverable <- **next step, handoff point is here**
- [ ] **M2** capability table . higher-half kernel (TTBR1) migration
- [ ] **M3** IPC
- [ ] **M4** first isolated virtio driver <- first real demo of fault isolation

## Handoff point: M1.5 making the vector table recoverable

**Current state**: the handler in `src/exceptions.rs` prints its report and then
halts (`semihost::exit`). `__exception_common` only puts the entry number into
x0 before the `bl`; it doesn't save registers.

**To do**:
1. Have `__exception_common` save x0-x30 to the stack to form an `ExceptionFrame`,
   and pass a pointer to the frame into the Rust handler
2. Change the handler to return a decision: "halt" or "skip this instruction and continue"
3. Implementation of "skip": `ELR_EL1 += 4`, restore registers, `eret`
4. Extend the demo so it survives three consecutive faults in a row:
   - read an unmapped address -> translation fault -> recover
   - write to `.rodata` -> **permission fault** (verifies M1's read-only mapping actually works) -> recover
   - the last one is unrecoverable, halts normally

**Why this matters**: this is the prototype for M4's driver isolation. Staying
alive after a fault is what makes it isolation. It's also **the only way to verify
that M1's permission bits are actually written correctly** -- with identity mapping,
incorrectly written permission bits don't otherwise show up.

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

## Review status

- M0 + M1 are committed (`0f6ce20`), as is the license (`26ef066`).
- Both cold review rounds above are complete.
- **No commits until every change has been cold-reviewed.**

## Conventions

- **Everything in this project is written in English** - source comments, kernel
  output strings, docs, commit messages. No other language anywhere in the tree.

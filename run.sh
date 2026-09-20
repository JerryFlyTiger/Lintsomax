#!/usr/bin/env bash
# cargo run passes the built ELF as the first argument
set -euo pipefail

KERNEL="${1:?usage: ./run.sh <kernel-elf> [run|debug|trace]}"
MODE="${2:-run}"

QEMU=(
  qemu-system-aarch64
  -M virt
  -cpu cortex-a72
  -m 128M
  -nographic
  -semihosting-config enable=on,target=native
  -kernel "$KERNEL"
)

case "$MODE" in
  run)   exec "${QEMU[@]}" ;;
  debug) echo "waiting for gdb on :1234 ... (run ./debug.sh in another terminal)"
         exec "${QEMU[@]}" -S -gdb tcp::1234 ;;
  trace) exec "${QEMU[@]}" -d int,guest_errors -D qemu-int.log ;;
  *)     echo "unknown mode: $MODE" >&2; exit 2 ;;
esac

#!/usr/bin/env bash
# cargo run 會把編好的 ELF 當第一個參數傳進來
set -euo pipefail

KERNEL="${1:?用法: ./run.sh <kernel-elf> [run|debug|trace]}"
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
  debug) echo "等待 gdb 連上 :1234 …（另開終端機跑 ./debug.sh）"
         exec "${QEMU[@]}" -S -gdb tcp::1234 ;;
  trace) exec "${QEMU[@]}" -d int,guest_errors -D qemu-int.log ;;
  *)     echo "未知模式: $MODE" >&2; exit 2 ;;
esac

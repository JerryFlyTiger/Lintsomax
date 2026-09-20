#!/usr/bin/env bash
# 先在另一個終端機跑： cargo run -- debug
set -euo pipefail
KERNEL="${1:-target/aarch64-unknown-none-softfloat/debug/lintsomax}"
exec aarch64-elf-gdb -q "$KERNEL"

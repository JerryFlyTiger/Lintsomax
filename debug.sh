#!/usr/bin/env bash
# First run in another terminal: cargo run -- debug
set -euo pipefail
KERNEL="${1:-target/aarch64-unknown-none-softfloat/debug/lintsomax}"
exec aarch64-elf-gdb -q "$KERNEL"

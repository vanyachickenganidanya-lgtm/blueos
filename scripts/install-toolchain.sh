#!/usr/bin/env bash
set -euo pipefail

if ! command -v rustup >/dev/null 2>&1; then
  echo "Installing rustup (minimal profile)..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  # shellcheck disable=SC1091
  source "${CARGO_HOME:-$HOME/.cargo}/env"
fi

rustup toolchain install stable --profile minimal
rustup default stable
rustup target add x86_64-unknown-none riscv64imac-unknown-none-elf

echo "Rust bare-metal toolchain is ready. Run: make all"

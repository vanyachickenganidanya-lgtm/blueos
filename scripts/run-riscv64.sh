#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="$ROOT/images/blueos-riscv64.elf"

command -v qemu-system-riscv64 >/dev/null || {
  echo "qemu-system-riscv64 is not installed (Debian/Ubuntu: sudo apt install qemu-system-misc)" >&2
  exit 1
}
[[ -f "$IMAGE" ]] || make -C "$ROOT" riscv64

exec qemu-system-riscv64 \
  -name BlueOS-riscv64 \
  -machine virt \
  -m 256M \
  -smp 1 \
  -bios default \
  -kernel "$IMAGE" \
  -device virtio-gpu-device \
  -netdev user,id=blueosnet \
  -device virtio-net-device,netdev=blueosnet,mac=52:54:00:12:34:57 \
  -serial mon:stdio \
  -no-reboot

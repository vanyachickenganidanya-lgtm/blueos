#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="$ROOT/images/blueos-x86_64.img"

command -v qemu-system-x86_64 >/dev/null || {
  echo "qemu-system-x86_64 is not installed (Debian/Ubuntu: sudo apt install qemu-system-x86)" >&2
  exit 1
}
[[ -f "$IMAGE" ]] || make -C "$ROOT" x86_64

exec qemu-system-x86_64 \
  -name BlueOS-x86_64 \
  -machine pc \
  -m 128M \
  -drive "format=raw,file=$IMAGE,if=ide,index=0" \
  -device VGA \
  -netdev user,id=blueosnet \
  -device e1000,netdev=blueosnet,mac=52:54:00:12:34:56 \
  -serial stdio \
  -no-reboot

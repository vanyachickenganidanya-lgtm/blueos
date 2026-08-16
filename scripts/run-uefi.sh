#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

image="images/blueos-x86_64.img"
[[ -f "$image" ]] || { echo "Missing $image; run make x86_64" >&2; exit 1; }

code="${OVMF_CODE:-}"
vars_template="${OVMF_VARS:-}"
if [[ -z "$code" ]]; then
  for candidate in /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd /usr/share/edk2/x64/OVMF_CODE.fd; do
    [[ -f "$candidate" ]] && { code="$candidate"; break; }
  done
fi
if [[ -z "$vars_template" ]]; then
  for candidate in /usr/share/OVMF/OVMF_VARS_4M.fd /usr/share/OVMF/OVMF_VARS.fd /usr/share/edk2/x64/OVMF_VARS.fd; do
    [[ -f "$candidate" ]] && { vars_template="$candidate"; break; }
  done
fi
[[ -n "$code" && -n "$vars_template" ]] || {
  echo "OVMF was not found. Install the 'ovmf' package or set OVMF_CODE and OVMF_VARS." >&2
  exit 1
}

mkdir -p build/uefi
cp "$vars_template" build/uefi/OVMF_VARS.fd
exec qemu-system-x86_64 \
  -machine q35 -m 256M \
  -drive if=pflash,format=raw,readonly=on,file="$code" \
  -drive if=pflash,format=raw,file=build/uefi/OVMF_VARS.fd \
  -drive format=raw,file="$image",if=ide,index=0 \
  -device VGA \
  -serial stdio -no-reboot

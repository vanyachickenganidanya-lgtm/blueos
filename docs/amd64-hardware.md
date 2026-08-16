# AMD64 hardware compatibility matrix

BlueOS targets the **AMD64 instruction set** and common PC standards. “AMD64” does not mean that every AMD-branded chipset or peripheral is automatically supported. The safest current physical-computer path is x86_64 UEFI: firmware supplies graphics, keyboard, pointer, storage and network protocols while native drivers are added incrementally.

Status meanings:

- **Implemented** — code is present and built by CI.
- **Partial** — useful support exists, with the limitation in the table.
- **Planned** — detected or in scope, but no usable native driver exists yet.

| Area | Interface / device | UEFI live workspace | Legacy BIOS kernel | Validation and limits |
|---|---|---:|---:|---|
| CPU | AMD64 long mode | Implemented | Implemented | QEMU x86_64 boot is smoke-tested. Physical AMD CPUs are not yet represented in an automated hardware lab. |
| Boot | UEFI x64 removable-media path | Implemented | — | Hybrid image contains `EFI/BOOT/BOOTX64.EFI`; Secure Boot is not supported because the image is unsigned. |
| Boot | Legacy MBR + custom stage loaders | — | Implemented | QEMU smoke-tested; stage 2 selects VBE mode and enters long mode. |
| Graphics | UEFI GOP RGB/BGR framebuffer | Implemented | — | Accepts direct RGB/BGR modes at 800×600 or above; bitmask and `PixelBltOnly` modes are rejected. No Radeon acceleration. |
| Graphics | VBE packed BGR framebuffer | — | Partial | QEMU standard VGA mode `0x118` is tested. Physical GPU VBE compatibility varies. |
| Keyboard | UEFI Simple Text Input | Implemented | — | Includes USB keyboards when firmware exposes them. BlueOS does not yet own an xHCI controller. |
| Keyboard | i8042/PS/2 set-1 polling | — | Partial | QEMU-tested; USB-only PCs need UEFI firmware input or legacy USB emulation. |
| Pointer | UEFI Simple Pointer | Partial | — | Relative movement and left-button dragging are implemented; firmware support varies. |
| Pointer | i8042 three-byte PS/2 mouse | — | Partial | QEMU/compatible controller path only. |
| Storage | UEFI Block I/O | Implemented | — | Enumerates whole media and performs 512-byte-block reads/writes. The installer only accepts a verified removable BlueOS source and explicitly selected writable internal target. |
| Storage | AHCI SATA | Planned | Planned | Can be reached through firmware Block I/O in UEFI when firmware provides it; no native AHCI command engine yet. |
| Storage | NVMe | Planned | Planned | Can be reached through firmware Block I/O in UEFI when firmware provides it; no native NVMe queue driver yet. |
| Storage | USB mass storage / xHCI | Partial | Planned | UEFI Block I/O can expose a boot USB. No native xHCI or USB stack yet. |
| Files | UEFI Simple File System | Implemented | — | Read-only root-directory listing; intentionally no create, write or delete operation. |
| Files | Native filesystem / VFS | Planned | Planned | The non-UEFI file view is currently a virtual read-only system tree. |
| Ethernet | UEFI Simple Network Protocol | Partial | — | Raw Ethernet, ARP, IPv4, ICMP, UDP and DNS are implemented. Static QEMU addressing is currently used; DHCP and physical LAN configuration are pending. |
| Ethernet | Intel e1000 PCI | — | Partial | QEMU e1000 is smoke-tested. The current native probe scans PCI bus 0 and uses identity-mapped MMIO. |
| Ethernet | RISC-V virtio-net MMIO | — | — | Implemented on the separate RISC-V `virt` target; not an AMD64 hardware path. |
| PCI | Configuration mechanism #1 | — | Partial | Bus-0 probing is used by e1000. Recursive bridges, ECAM/MMCONFIG and a user-visible inventory are pending. |
| USB | Native xHCI host controller | Planned | Planned | Firmware input/storage protocols are the current UEFI bridge; no native transfer rings, hubs or HID class driver. |
| Audio | HDA / USB audio | Planned | Planned | No mixer or audio output driver. |
| Wi-Fi | PCIe/USB WLAN | Planned | Planned | No radio firmware loader, WPA supplicant or WLAN MAC layer. |
| AMD GPU | Native Radeon display/acceleration | Planned | Planned | GOP/VBE is deliberately the first physical-hardware graphics path. |
| Platform | ACPI, APIC, SMP, AMD IOMMU | Planned | Planned | Current MVP is single-core polling and does not claim power-management or IOMMU support. |

## Safe physical test order

1. Write the entire hybrid image to a spare USB drive in raw/DD mode.
2. Disable Secure Boot, leave UEFI enabled, and disconnect disks containing valuable data.
3. Confirm the desktop, keyboard, pointer, clock and **read-only** file view before opening the installer.
4. Check whether the firmware exposes GOP, Block I/O and Simple Network Protocol. A missing firmware protocol is reported as unavailable rather than replaced by an unsafe guess.
5. Reconnect only a disposable target disk. Use `INSTALL`, `INSTALL SELECT N`, and finally the exact destructive confirmation `INSTALL ERASE`.
6. Record the motherboard, CPU, firmware version, GPU, storage controller, NIC, which protocols appeared, and the first failing step when reporting compatibility.

“Implemented” in this matrix is not a claim of universal compatibility. Physical status should only be upgraded after a reproducible test on the named machine or controller.

SHELL := /bin/bash

X86_TARGET := x86_64-unknown-none
RISCV_TARGET := riscv64imac-unknown-none-elf
PROFILE := release
CARGO := cargo
OUT := build
IMAGES := images
STAGE2_SECTORS := 16

.PHONY: all x86_64 riscv64 clean run-x86_64 run-riscv64 check tools

all: x86_64 riscv64

check: all
	@test "$$(stat -c%s $(IMAGES)/blueos-x86_64.img)" -ge 1024
	@readelf -h $(IMAGES)/blueos-riscv64.elf | grep -q 'RISC-V'
	@echo "BlueOS images passed structural checks."

tools:
	@command -v $(CARGO) >/dev/null || { echo "cargo is missing; run scripts/install-toolchain.sh"; exit 1; }
	@command -v as >/dev/null
	@command -v ld >/dev/null
	@command -v objcopy >/dev/null

x86_64: tools
	@mkdir -p $(OUT)/x86 $(IMAGES)
	$(CARGO) build --$(PROFILE) --target $(X86_TARGET)
	objcopy -O binary target/$(X86_TARGET)/$(PROFILE)/blueos $(OUT)/x86/kernel.bin
	@kernel_size=$$(stat -c%s $(OUT)/x86/kernel.bin); \
	 kernel_sectors=$$(( (kernel_size + 511) / 512 )); \
	 if (( kernel_sectors > 1536 )); then echo "x86 kernel is too large for the BIOS bounce loader"; exit 1; fi; \
	 echo "x86 kernel: $$kernel_size bytes ($$kernel_sectors sectors)"; \
	 as --32 --defsym KERNEL_SECTORS=$$kernel_sectors -o $(OUT)/x86/stage2.o boot/x86/stage2.S; \
	 ld -m elf_i386 -T boot/x86/stage2.ld -o $(OUT)/x86/stage2.bin $(OUT)/x86/stage2.o; \
	 stage2_size=$$(stat -c%s $(OUT)/x86/stage2.bin); \
	 if (( stage2_size > $(STAGE2_SECTORS) * 512 )); then echo "stage2 exceeds its fixed slot"; exit 1; fi; \
	 dd if=/dev/zero of=$(OUT)/x86/stage2.padded bs=512 count=$(STAGE2_SECTORS) status=none; \
	 dd if=$(OUT)/x86/stage2.bin of=$(OUT)/x86/stage2.padded conv=notrunc status=none; \
	 dd if=/dev/zero of=$(OUT)/x86/kernel.padded bs=512 count=$$kernel_sectors status=none; \
	 dd if=$(OUT)/x86/kernel.bin of=$(OUT)/x86/kernel.padded conv=notrunc status=none
	as --32 -o $(OUT)/x86/stage1.o boot/x86/stage1.S
	ld -m elf_i386 -T boot/x86/stage1.ld -o $(OUT)/x86/stage1.bin $(OUT)/x86/stage1.o
	@test "$$(stat -c%s $(OUT)/x86/stage1.bin)" -eq 512
	cat $(OUT)/x86/stage1.bin $(OUT)/x86/stage2.padded $(OUT)/x86/kernel.padded > $(IMAGES)/blueos-x86_64.img
	cp target/$(X86_TARGET)/$(PROFILE)/blueos $(IMAGES)/blueos-x86_64.elf
	@echo "Built $(IMAGES)/blueos-x86_64.img"

riscv64: tools
	@mkdir -p $(OUT)/riscv64 $(IMAGES)
	$(CARGO) build --$(PROFILE) --target $(RISCV_TARGET)
	cp target/$(RISCV_TARGET)/$(PROFILE)/blueos $(IMAGES)/blueos-riscv64.elf
	@echo "Built $(IMAGES)/blueos-riscv64.elf (OpenSBI/QEMU kernel image)"

run-x86_64: x86_64
	./scripts/run-x86_64.sh

run-riscv64: riscv64
	./scripts/run-riscv64.sh

clean:
	rm -rf $(OUT) target $(IMAGES)/*.img $(IMAGES)/*.elf

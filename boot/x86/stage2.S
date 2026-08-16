/*
 * BlueOS BIOS/VBE loader.
 *
 * KERNEL_SECTORS is supplied by Makefile. The kernel follows the boot
 * sector and the 16-sector second stage. It is read below 1 MiB, copied
 * to 1 MiB, and entered with the first 4 GiB identity mapped.
 */
.code16
.section .text
.global _start

.set STAGE2_SECTORS, 16
.set KERNEL_LBA, (1 + STAGE2_SECTORS)
.set BOOT_INFO, 0x7200
.set VBE_INFO,  0x7000

_start:
    cli
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movw $0x7a00, %sp
    sti
    cld
    movb %dl, boot_drive

    movw $loading, %si
    call puts

    /* Fast A20 gate. */
    inb $0x92, %al
    orb $0x02, %al
    andb $0xfe, %al
    outb %al, $0x92

    /* Initialize BootInfo, then ask for VBE 1024x768x24 LFB mode 0x118. */
    movl $0x534f554c, BOOT_INFO       /* "LUOS" low half of magic */
    movl $0x45554c42, BOOT_INFO + 4   /* "BLUE" high half */
    xorw %ax, %ax
    movw %ax, %es
    movw $VBE_INFO, %di
    movw $0x4f01, %ax
    movw $0x0118, %cx
    int $0x10
    cmpw $0x004f, %ax
    jne no_graphics

    movw $0x4f02, %ax
    movw $0x4118, %bx              /* mode + linear framebuffer bit */
    int $0x10
    cmpw $0x004f, %ax
    jne no_graphics

    /* BootInfo layout is shared with src/arch/x86_64.rs. */
    movl VBE_INFO + 40, %eax
    movl %eax, BOOT_INFO + 8
    movl $0, BOOT_INFO + 12
    movzwl VBE_INFO + 18, %eax
    movl %eax, BOOT_INFO + 16
    movzwl VBE_INFO + 20, %eax
    movl %eax, BOOT_INFO + 20
    movzwl VBE_INFO + 16, %eax
    movl %eax, BOOT_INFO + 24
    movzbl VBE_INFO + 25, %eax
    movl %eax, BOOT_INFO + 28
    jmp load_kernel

no_graphics:
    movl $0, BOOT_INFO + 8
    movl $0, BOOT_INFO + 12
    movl $0, BOOT_INFO + 16
    movl $0, BOOT_INFO + 20
    movl $0, BOOT_INFO + 24
    movl $0, BOOT_INFO + 28

load_kernel:
    movb boot_drive, %al
    movb %al, BOOT_INFO + 32
    movw $KERNEL_SECTORS, sectors_left
    movl $KERNEL_LBA, current_lba
    movw $0x1000, load_segment

read_loop:
    cmpw $0, sectors_left
    je kernel_loaded

    movw sectors_left, %ax
    cmpw $64, %ax
    jbe chunk_ready
    movw $64, %ax
chunk_ready:
    movw %ax, dap_count
    movw load_segment, %bx
    movw %bx, dap_segment
    movl current_lba, %ebx
    movl %ebx, dap_lba

    movw $dap, %si
    movb boot_drive, %dl
    movb $0x42, %ah
    int $0x13
    jc disk_error

    movzwl dap_count, %eax
    subw %ax, sectors_left
    addl %eax, current_lba
    shlw $5, %ax                  /* 512 bytes = 0x20 paragraphs */
    addw %ax, load_segment
    jmp read_loop

kernel_loaded:
    cli
    lgdt gdt_descriptor
    movl %cr0, %eax
    orl $1, %eax
    movl %eax, %cr0
    ljmp $0x08, $protected_entry

disk_error:
    movw $error, %si
    call puts
1:
    hlt
    jmp 1b

puts:
    lodsb
    testb %al, %al
    jz 2f
    movb $0x0e, %ah
    movw $0x0007, %bx
    int $0x10
    jmp puts
2:
    ret

/* -------------------------- 32-bit setup -------------------------- */
.code32
protected_entry:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movl $0x00090000, %esp
    cld

    /* Copy the sector-padded kernel from the BIOS bounce area. */
    movl $0x00010000, %esi
    movl $0x00100000, %edi
    movl $(KERNEL_SECTORS * 128), %ecx
    rep movsl

    /* PML4 + PDPT + four page directories, covering physical 0..4 GiB. */
    xorl %eax, %eax
    movl $0x00001000, %edi
    movl $(0x6000 / 4), %ecx
    rep stosl

    movl $0x00002003, 0x00001000
    movl $0x00003003, 0x00002000
    movl $0x00004003, 0x00002008
    movl $0x00005003, 0x00002010
    movl $0x00006003, 0x00002018

    movl $0x00003000, %edi
    xorl %ecx, %ecx
map_page:
    movl %ecx, %eax
    shll $21, %eax
    orl $0x83, %eax               /* present, writable, 2 MiB page */
    movl %eax, (%edi)
    movl $0, 4(%edi)
    addl $8, %edi
    incl %ecx
    cmpl $2048, %ecx
    jne map_page

    movl %cr4, %eax
    orl $(1 << 5), %eax           /* PAE */
    movl %eax, %cr4
    movl $0x00001000, %eax
    movl %eax, %cr3

    movl $0xc0000080, %ecx
    rdmsr
    orl $(1 << 8), %eax           /* EFER.LME */
    wrmsr

    movl %cr0, %eax
    orl $(1 << 31), %eax          /* paging */
    movl %eax, %cr0
    ljmp $0x18, $long_mode_entry

/* -------------------------- 64-bit entry -------------------------- */
.code64
long_mode_entry:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movq $0x00090000, %rsp
    xorq %rbp, %rbp
    movq $BOOT_INFO, %rdi
    movq $0x00100000, %rax
    jmp *%rax

/* ----------------------------- data ------------------------------- */
.code16
boot_drive:   .byte 0
sectors_left: .word 0
load_segment: .word 0
current_lba:  .long 0
loading:      .asciz "VBE and kernel...\r\n"
error:        .asciz "Kernel disk read failed\r\n"

.align 4
dap:
    .byte 0x10, 0
dap_count:
    .word 0
dap_offset:
    .word 0
dap_segment:
    .word 0
dap_lba:
    .quad 0

.align 8
gdt:
    .quad 0x0000000000000000
    .quad 0x00cf9a000000ffff       /* 0x08: 32-bit code */
    .quad 0x00cf92000000ffff       /* 0x10: data */
    .quad 0x00af9a000000ffff       /* 0x18: 64-bit code */
gdt_end:
gdt_descriptor:
    .word gdt_end - gdt - 1
    .long gdt

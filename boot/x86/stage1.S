/* BlueOS BIOS sector. Loads the fixed-size second stage at 0000:8000. */
.code16
.section .text
.global _start

.set STAGE2_SECTORS, 16

_start:
    cli
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movw $0x7c00, %sp
    sti
    cld

    movb %dl, boot_drive
    movw $banner, %si
    call puts

    /* Require INT 13h extensions; QEMU/SeaBIOS provides them. */
    movw $0x55aa, %bx
    movb $0x41, %ah
    movb boot_drive, %dl
    int $0x13
    jc disk_error
    cmpw $0xaa55, %bx
    jne disk_error

    movb boot_drive, %dl
    movw $dap, %si
    movb $0x42, %ah
    int $0x13
    jc disk_error

    movb boot_drive, %dl
    ljmp $0x0000, $0x8000

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

boot_drive:
    .byte 0
banner:
    .asciz "BlueOS stage1...\r\n"
error:
    .asciz "Disk read failed\r\n"

.align 4
dap:
    .byte 0x10, 0
    .word STAGE2_SECTORS
    .word 0x8000
    .word 0x0000
    .quad 1

.org 510
.word 0xaa55

.section .text.entry
.globl _start
.align 4
_start:
    /* OpenSBI enters in supervisor mode: a0=hart id, a1=DTB address. */
    la      sp, __stack_top
    mv      s0, a0
    mv      s1, a1

    la      t0, __bss_start
    la      t1, __bss_end
1:
    bgeu    t0, t1, 2f
    sd      zero, 0(t0)
    addi    t0, t0, 8
    j       1b
2:
    mv      a0, s0
    mv      a1, s1
    call    riscv_main
3:
    wfi
    j       3b

.section .text.vector
.global vector

.equ REG_IRQ_WAITFLAGS, 0x03007ff8
.equ REG_INTERRUPT_VECTOR, 0x03007ffc
.equ REG_IF, 0x04000202
.equ FAST_EXIT, 0x040003fc
.equ NORMAL_CONTEXTIDR, 0x00000001
.equ HBLANK_CONTEXTIDR, 0x00000002
.equ HBLANK_TTBR0, 0x0f00404a

.org 0x0000
vector:
    b reset
    b undefined_handler
    b swi_handler
    b prefetch_abort_handler
    b data_abort_handler
    b reserved_handler
    b irq_handler
    b fiq_handler

.org 0x0040
reset:
reset_loop:
    b reset_loop

undefined_handler:
undefined_loop:
    b undefined_loop

prefetch_abort_handler:
prefetch_abort_loop:
    b prefetch_abort_loop

data_abort_handler:
data_abort_loop:
    b data_abort_loop

reserved_handler:
reserved_loop:
    b reserved_loop

fiq_handler:
fiq_loop:
    b fiq_loop

.org 0x0100
swi_handler:
    ldrh r12, [lr, #-2]
    and r12, r12, #0xff
    cmp r12, #4
    beq intr_wait
    cmp r12, #5
    beq vblank_intr_wait
    cmp r12, #6
    bne swi_return

div:
    mov r2, #0
    mov r3, #0
    cmp r1, #0
    beq div_done
    mov r3, r0
div_loop:
    cmp r3, r1
    blo div_done
    sub r3, r3, r1
    add r2, r2, #1
    b div_loop
div_done:
    mov r0, r2
    mov r1, r3
    mov r3, r2
swi_return:
    movs pc, lr

vblank_intr_wait:
    mov r0, #1
    mov r1, #1
intr_wait:
    ldr r2, reg_irq_waitflags_ptr
    mrs r3, spsr
    stmdb sp!, {r3, lr}
    cmp r0, #0
    beq intr_wait_loop
    ldrh r3, [r2]
    bic r3, r3, r1
    strh r3, [r2]
intr_wait_loop:
    mrs r3, cpsr
    bic r3, r3, #0x80
    msr cpsr_c, r3
    wfi
    mrs r3, cpsr
    orr r3, r3, #0x80
    msr cpsr_c, r3
    ldrh r3, [r2]
    tst r3, r1
    beq intr_wait_loop
    ldmia sp!, {r3, lr}
    msr spsr, r3
    movs pc, lr
reg_irq_waitflags_ptr:
    .word REG_IRQ_WAITFLAGS

.org 0x0200
irq_handler:
    stmdb sp!, {r0-r3,r12,lr}
    ldr r0, reg_if_ptr
    ldrh r1, [r0]
    tst r1, #1
    bne irq_handler_slow
    tst r1, #2
    bne irq_handler_fast_hblank

irq_handler_slow:
    ldr r0, reg_interrupt_vector_ptr
    add lr, pc, #0
    ldr pc, [r0]
irq_handler_return:
    ldmia sp!, {r0-r3,r12,lr}
    subs pc, lr, #4

irq_handler_fast_hblank:
    mrc p15, 0, r2, c2, c0, 0
    mrc p15, 0, r3, c13, c0, 1
    stmdb sp!, {r2-r3}
    ldr r0, hblank_contextidr_ptr
    mcr p15, 0, r0, c13, c0, 1
    ldr r0, hblank_ttbr0_ptr
    mcr p15, 0, r0, c2, c0, 0
    mov r0, #0
    mcr p15, 0, r0, c8, c7, 0
    dsb sy
    isb sy

    ldr r0, reg_interrupt_vector_ptr
    add lr, pc, #0
    ldr pc, [r0]

    ldmia sp!, {r2-r3}
    mcr p15, 0, r3, c13, c0, 1
    mcr p15, 0, r2, c2, c0, 0
    mov r0, #0
    mcr p15, 0, r0, c8, c7, 0
    dsb sy
    isb sy

    ldr r0, fast_exit_ptr
    mov r1, #1
    str r1, [r0]
    b irq_handler_return

reg_interrupt_vector_ptr:
    .word REG_INTERRUPT_VECTOR
reg_if_ptr:
    .word REG_IF
fast_exit_ptr:
    .word FAST_EXIT
hblank_contextidr_ptr:
    .word HBLANK_CONTEXTIDR
hblank_ttbr0_ptr:
    .word HBLANK_TTBR0

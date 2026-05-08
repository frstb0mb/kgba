.section .text.vector
.global vector

.equ REG_IRQ_WAITFLAGS, 0x03007ff8
.equ REG_INTERRUPT_VECTOR, 0x03007ffc

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
    movs pc, lr
reg_irq_waitflags_ptr:
    .word REG_IRQ_WAITFLAGS

.org 0x0200
irq_handler:
    stmdb sp!, {r0-r3,r12,lr}
    ldr r0, reg_interrupt_vector_ptr
    add lr, pc, #0
    ldr pc, [r0]
    ldmia sp!, {r0-r3,r12,lr}
    subs pc, lr, #4
reg_interrupt_vector_ptr:
    .word REG_INTERRUPT_VECTOR

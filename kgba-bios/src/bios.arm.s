.section .text.vector
.global vector

.equ REG_IRQ_WAITFLAGS, 0x03007ff8
.equ REG_INTERRUPT_VECTOR, 0x03007ffc
.equ REG_IF, 0x04000202
.equ REG_IME, 0x04000208
.equ REG_BG0HOFS, 0x04000010
.equ REG_BG0VOFS, 0x04000012
.equ REG_BG1HOFS, 0x04000014
.equ REG_BG1VOFS, 0x04000016
.equ REG_BG2HOFS, 0x04000018
.equ REG_BG2VOFS, 0x0400001a
.equ REG_BG3HOFS, 0x0400001c
.equ REG_BG3VOFS, 0x0400001e
.equ FAST_HBLANK_STATE, 0x0f00a000
.equ SHADOW_IO, 0x0f009000
.equ SHADOW_IF, SHADOW_IO + 0x0202
.equ SHADOW_IME, SHADOW_IO + 0x0208
.equ SHADOW_BG0HOFS, SHADOW_IO + 0x0010
.equ SHADOW_BG0VOFS, SHADOW_IO + 0x0012
.equ SHADOW_BG1HOFS, SHADOW_IO + 0x0014
.equ SHADOW_BG1VOFS, SHADOW_IO + 0x0016
.equ SHADOW_BG2HOFS, SHADOW_IO + 0x0018
.equ SHADOW_BG2VOFS, SHADOW_IO + 0x001a
.equ SHADOW_BG3HOFS, SHADOW_IO + 0x001c
.equ SHADOW_BG3VOFS, SHADOW_IO + 0x001e
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
    cmp r12, #0
    bne swi_dispatch
    ldr r12, [lr, #-4]
    and r12, r12, #0xff
    cmp r12, #0
    bne swi_dispatch
    ldr r12, [lr, #-4]
    mov r12, r12, lsr #16
    and r12, r12, #0xff
swi_dispatch:
    cmp r12, #4
    beq intr_wait
    cmp r12, #5
    beq vblank_intr_wait
    cmp r12, #6
    bne swi_return

div:
    cmp r1, #0
    beq div_by_zero
    mov r2, #0
    mov r3, #0
    cmp r0, #0
    rsblt r0, r0, #0
    orrlt r3, r3, #2
    eorlt r3, r3, #1
    cmp r1, #0
    rsblt r1, r1, #0
    eorlt r3, r3, #1
    mov r12, #1
div_align_loop:
    cmp r1, r0, lsr #1
    movls r1, r1, lsl #1
    movls r12, r12, lsl #1
    bls div_align_loop
div_loop:
    cmp r0, r1
    subcs r0, r0, r1
    orrcs r2, r2, r12
    movs r12, r12, lsr #1
    movne r1, r1, lsr #1
    bne div_loop
    tst r3, #1
    rsbne r2, r2, #0
    tst r3, #2
    rsbne r0, r0, #0
    mov r1, r0
    mov r0, r2
    cmp r2, #0
    rsblt r3, r2, #0
    movge r3, r2
    movs pc, lr

div_by_zero:
    mov r1, r0
    mov r3, #0
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
intr_wait_poll:
    ldrh r3, [r2]
    tst r3, r1
    beq intr_wait_poll
    mrs r3, cpsr
    orr r3, r3, #0x80
    msr cpsr_c, r3
    ldmia sp!, {r3, lr}
    msr spsr, r3
    movs pc, lr
reg_irq_waitflags_ptr:
    .word REG_IRQ_WAITFLAGS

.org 0x0240
irq_handler:
    stmdb sp!, {r0-r3,r12,lr}
    ldr r0, reg_if_ptr
    ldrh r1, [r0]
    cmp r1, #0
    beq irq_handler_return
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

    ldr r12, fast_hblank_state_ptr
    mov r2, #0
    str r2, [r12]
    ldr r2, hblank_dirty_mask
    str r2, [r12, #24]
    ldr r0, shadow_bg0hofs_ptr
    ldrh r1, [r0]
    strh r1, [r12, #28]
    ldr r0, shadow_bg1hofs_ptr
    ldrh r1, [r0]
    strh r1, [r12, #30]
    ldr r0, shadow_bg2hofs_ptr
    ldrh r1, [r0]
    strh r1, [r12, #32]
    ldr r0, shadow_bg3hofs_ptr
    ldrh r1, [r0]
    strh r1, [r12, #34]
    ldr r0, shadow_bg0vofs_ptr
    ldrh r1, [r0]
    strh r1, [r12, #36]
    ldr r0, shadow_bg1vofs_ptr
    ldrh r1, [r0]
    strh r1, [r12, #38]
    ldr r0, shadow_bg2vofs_ptr
    ldrh r1, [r0]
    strh r1, [r12, #40]
    ldr r0, shadow_bg3vofs_ptr
    ldrh r1, [r0]
    strh r1, [r12, #42]
    ldr r0, shadow_ime_ptr
    ldrh r1, [r0]
    strh r1, [r12, #44]
    ldr r0, shadow_if_ptr
    ldrh r1, [r0]
    orr r1, r1, #2
    strh r1, [r12, #46]
    ldr r0, [r12, #4]
    ldr r1, [r12, #8]
    dsb sy
    str r0, [r12, #12]
    str r1, [r12, #16]
    dsb sy

    ldmia sp!, {r2-r3}
    mcr p15, 0, r3, c13, c0, 1
    mcr p15, 0, r2, c2, c0, 0
    mov r0, #0
    mcr p15, 0, r0, c8, c7, 0
    dsb sy
    isb sy

    b irq_handler_return

reg_interrupt_vector_ptr:
    .word REG_INTERRUPT_VECTOR
reg_if_ptr:
    .word REG_IF
shadow_if_ptr:
    .word SHADOW_IF
shadow_ime_ptr:
    .word SHADOW_IME
shadow_bg0hofs_ptr:
    .word SHADOW_BG0HOFS
shadow_bg0vofs_ptr:
    .word SHADOW_BG0VOFS
shadow_bg1hofs_ptr:
    .word SHADOW_BG1HOFS
shadow_bg1vofs_ptr:
    .word SHADOW_BG1VOFS
shadow_bg2hofs_ptr:
    .word SHADOW_BG2HOFS
shadow_bg2vofs_ptr:
    .word SHADOW_BG2VOFS
shadow_bg3hofs_ptr:
    .word SHADOW_BG3HOFS
shadow_bg3vofs_ptr:
    .word SHADOW_BG3VOFS
fast_hblank_state_ptr:
    .word FAST_HBLANK_STATE
hblank_dirty_mask:
    .word 0x000003ff
hblank_contextidr_ptr:
    .word HBLANK_CONTEXTIDR
hblank_ttbr0_ptr:
    .word HBLANK_TTBR0

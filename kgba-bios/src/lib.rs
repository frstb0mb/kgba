#![no_std]

pub const BIOS_SIZE: usize = 0x4000;

const BIOS_RESET_HANDLER_OFFSET: usize = 0x40;
const BIOS_SWI_HANDLER_OFFSET: usize = 0x100;

const BIOS_VECTOR_TABLE: [u32; 8] = [
    0xea00_000e, // reset -> 0x40
    0xeaff_fffe, // undefined instruction
    0xea00_003c, // swi -> 0x100
    0xeaff_fffe, // prefetch abort
    0xeaff_fffe, // data abort
    0xeaff_fffe, // reserved
    0xeaff_fffe, // irq
    0xeaff_fffe, // fiq
];

const BIOS_RESET_HANDLER: [u32; 1] = [
    0xeaff_fffe, // b .
];

const BIOS_SWI_HANDLER: [u32; 14] = [
    0xe3a0_2000, // mov r2, #0
    0xe3a0_3000, // mov r3, #0
    0xe351_0000, // cmp r1, #0
    0x0a00_0005, // beq done
    0xe1a0_3000, // mov r3, r0
    0xe153_0001, // cmp r3, r1
    0x3a00_0002, // blo done
    0xe043_3001, // sub r3, r3, r1
    0xe282_2001, // add r2, r2, #1
    0xeaff_fffa, // b loop
    0xe1a0_0002, // mov r0, r2
    0xe1a0_1003, // mov r1, r3
    0xe1a0_3002, // mov r3, r2
    0xe1b0_f00e, // movs pc, lr
];

pub const DEFAULT_BIOS_IMAGE: [u8; BIOS_SIZE] = build_default_bios_image();

pub const fn default_bios_image() -> [u8; BIOS_SIZE] {
    DEFAULT_BIOS_IMAGE
}

const fn build_default_bios_image() -> [u8; BIOS_SIZE] {
    let mut image = [0; BIOS_SIZE];
    image = write_words(image, 0, &BIOS_VECTOR_TABLE);
    image = write_words(image, BIOS_RESET_HANDLER_OFFSET, &BIOS_RESET_HANDLER);
    image = write_words(image, BIOS_SWI_HANDLER_OFFSET, &BIOS_SWI_HANDLER);
    image
}

const fn write_words(mut image: [u8; BIOS_SIZE], offset: usize, words: &[u32]) -> [u8; BIOS_SIZE] {
    let mut index = 0;
    while index < words.len() {
        let bytes = words[index].to_le_bytes();
        let byte_offset = offset + index * 4;
        image[byte_offset] = bytes[0];
        image[byte_offset + 1] = bytes[1];
        image[byte_offset + 2] = bytes[2];
        image[byte_offset + 3] = bytes[3];
        index += 1;
    }
    image
}

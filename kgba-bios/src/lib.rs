#![no_std]

pub const BIOS_SIZE: usize = 0x4000;

include!(concat!(env!("OUT_DIR"), "/bios_image.rs"));

pub const fn default_bios_image() -> [u8; BIOS_SIZE] {
    DEFAULT_BIOS_IMAGE
}

use std::{fs, io, path::Path};

use super::memory_map::GAME_PAK_ROM_START;

#[derive(Debug)]
pub struct Cartridge {
    rom: Vec<u8>,
}

impl Cartridge {
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let rom = fs::read(path)?;
        Ok(Self { rom })
    }

    pub fn from_bytes(rom: Vec<u8>) -> Self {
        Self { rom }
    }

    pub fn rom(&self) -> &[u8] {
        &self.rom
    }

    pub fn read_u16(&self, addr: u32) -> u16 {
        let offset = addr.wrapping_sub(GAME_PAK_ROM_START) as usize;
        if offset + 1 >= self.rom.len() {
            return 0xffff;
        }
        u16::from_le_bytes([self.rom[offset], self.rom[offset + 1]])
    }

    pub fn read_u32(&self, addr: u32) -> u32 {
        u32::from(self.read_u16(addr)) | (u32::from(self.read_u16(addr + 2)) << 16)
    }

    pub fn entry_thumb_addr(&self) -> Option<u32> {
        let value = self.read_u32(GAME_PAK_ROM_START + 0x1f4);
        if value & 1 != 0 {
            Some(value & !1)
        } else {
            None
        }
    }
}

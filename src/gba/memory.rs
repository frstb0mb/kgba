use super::memory_map::{EWRAM_SIZE, IWRAM_SIZE, OAM_SIZE, PALETTE_SIZE, VRAM_SIZE};

#[derive(Debug)]
pub struct GbaMemory {
    ewram: Box<[u8; EWRAM_SIZE]>,
    iwram: Box<[u8; IWRAM_SIZE]>,
    palette: Box<[u8; PALETTE_SIZE]>,
    vram: Box<[u8; VRAM_SIZE]>,
    oam: Box<[u8; OAM_SIZE]>,
}

impl GbaMemory {
    pub fn new() -> Self {
        Self {
            ewram: Box::new([0; EWRAM_SIZE]),
            iwram: Box::new([0; IWRAM_SIZE]),
            palette: Box::new([0; PALETTE_SIZE]),
            vram: Box::new([0; VRAM_SIZE]),
            oam: Box::new([0; OAM_SIZE]),
        }
    }

    pub fn ewram(&self) -> &[u8] {
        self.ewram.as_ref()
    }

    pub fn ewram_mut(&mut self) -> &mut [u8] {
        self.ewram.as_mut()
    }

    pub fn iwram(&self) -> &[u8] {
        self.iwram.as_ref()
    }

    pub fn iwram_mut(&mut self) -> &mut [u8] {
        self.iwram.as_mut()
    }

    pub fn palette(&self) -> &[u8] {
        self.palette.as_ref()
    }

    pub fn vram(&self) -> &[u8] {
        self.vram.as_ref()
    }

    pub fn vram_mut(&mut self) -> &mut [u8] {
        self.vram.as_mut()
    }

    pub fn oam(&self) -> &[u8] {
        self.oam.as_ref()
    }

    pub fn oam_mut(&mut self) -> &mut [u8] {
        self.oam.as_mut()
    }

    pub fn write_vram_halfword(&mut self, byte_offset: usize, value: u16) {
        let bytes = value.to_le_bytes();
        self.vram[byte_offset] = bytes[0];
        self.vram[byte_offset + 1] = bytes[1];
    }

    pub fn write_palette_halfword(&mut self, byte_offset: usize, value: u16) {
        let bytes = value.to_le_bytes();
        self.palette[byte_offset] = bytes[0];
        self.palette[byte_offset + 1] = bytes[1];
    }

    pub fn read_halfword(&self, region: MemoryRegionKind, byte_offset: usize) -> u16 {
        read_u16(self.region(region), byte_offset)
    }

    pub fn write_halfword(&mut self, region: MemoryRegionKind, byte_offset: usize, value: u16) {
        write_u16(self.region_mut(region), byte_offset, value);
    }

    pub fn read_ewram_word(&self, byte_offset: usize) -> u32 {
        read_u32(self.ewram.as_ref(), byte_offset)
    }

    pub fn write_ewram_word(&mut self, byte_offset: usize, value: u32) {
        write_u32(self.ewram.as_mut(), byte_offset, value);
    }

    pub fn read_iwram_word(&self, byte_offset: usize) -> u32 {
        read_u32(self.iwram.as_ref(), byte_offset)
    }

    pub fn write_iwram_word(&mut self, byte_offset: usize, value: u32) {
        write_u32(self.iwram.as_mut(), byte_offset, value);
    }

    fn region(&self, region: MemoryRegionKind) -> &[u8] {
        match region {
            MemoryRegionKind::Ewram => self.ewram.as_ref(),
            MemoryRegionKind::Iwram => self.iwram.as_ref(),
            MemoryRegionKind::Palette => self.palette.as_ref(),
            MemoryRegionKind::Vram => self.vram.as_ref(),
            MemoryRegionKind::Oam => self.oam.as_ref(),
        }
    }

    fn region_mut(&mut self, region: MemoryRegionKind) -> &mut [u8] {
        match region {
            MemoryRegionKind::Ewram => self.ewram.as_mut(),
            MemoryRegionKind::Iwram => self.iwram.as_mut(),
            MemoryRegionKind::Palette => self.palette.as_mut(),
            MemoryRegionKind::Vram => self.vram.as_mut(),
            MemoryRegionKind::Oam => self.oam.as_mut(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRegionKind {
    Ewram,
    Iwram,
    Palette,
    Vram,
    Oam,
}

fn read_u16(memory: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([memory[offset], memory[offset + 1]])
}

fn write_u16(memory: &mut [u8], offset: usize, value: u16) {
    let bytes = value.to_le_bytes();
    memory[offset] = bytes[0];
    memory[offset + 1] = bytes[1];
}

fn read_u32(memory: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        memory[offset],
        memory[offset + 1],
        memory[offset + 2],
        memory[offset + 3],
    ])
}

fn write_u32(memory: &mut [u8], offset: usize, value: u32) {
    let bytes = value.to_le_bytes();
    memory[offset] = bytes[0];
    memory[offset + 1] = bytes[1];
    memory[offset + 2] = bytes[2];
    memory[offset + 3] = bytes[3];
}

impl Default for GbaMemory {
    fn default() -> Self {
        Self::new()
    }
}

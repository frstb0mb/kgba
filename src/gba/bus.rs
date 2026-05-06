use super::{
    memory::GbaMemory,
    memory_map::{
        BG0CNT, BG0HOFS, BG0VOFS, BG1CNT, BG1HOFS, BG1VOFS, BG2CNT, BG2HOFS, BG2VOFS, BG3CNT,
        BG3HOFS, BG3VOFS, DISPCNT, DISPSTAT, IO_START, KEYINPUT, VCOUNT,
    },
    ppu::{FrameBuffer, Ppu},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessSize {
    Byte,
    Halfword,
    Word,
}

#[derive(Debug)]
pub struct Bus<'a> {
    memory: &'a mut GbaMemory,
    ppu: Ppu,
}

impl<'a> Bus<'a> {
    pub fn new(memory: &'a mut GbaMemory) -> Self {
        Self {
            memory,
            ppu: Ppu::new(),
        }
    }

    pub fn memory_mut(&mut self) -> &mut GbaMemory {
        self.memory
    }

    pub fn ppu(&self) -> &Ppu {
        &self.ppu
    }

    pub fn ppu_mut(&mut self) -> &mut Ppu {
        &mut self.ppu
    }

    pub fn read(&mut self, addr: u32, size: AccessSize) -> u32 {
        match size {
            AccessSize::Byte => self.read_byte(addr).into(),
            AccessSize::Halfword => self.read_halfword(addr).into(),
            AccessSize::Word => {
                u32::from(self.read_halfword(addr))
                    | (u32::from(self.read_halfword(addr + 2)) << 16)
            }
        }
    }

    pub fn write(&mut self, addr: u32, size: AccessSize, value: u32) {
        match size {
            AccessSize::Byte => self.write_byte(addr, value as u8),
            AccessSize::Halfword => self.write_halfword(addr, value as u16),
            AccessSize::Word => {
                self.write_halfword(addr, value as u16);
                self.write_halfword(addr + 2, (value >> 16) as u16);
            }
        }
    }

    pub fn render_frame_argb8888(&self) -> FrameBuffer {
        self.ppu
            .render_frame(self.memory.palette(), self.memory.vram(), self.memory.oam())
    }

    fn read_byte(&mut self, addr: u32) -> u8 {
        let aligned = addr & !1;
        let value = self.read_halfword(aligned);
        if addr & 1 == 0 {
            value as u8
        } else {
            (value >> 8) as u8
        }
    }

    fn read_halfword(&mut self, addr: u32) -> u16 {
        match addr {
            DISPCNT => self.ppu.dispcnt(),
            DISPSTAT => self.ppu.dispstat(),
            VCOUNT => self.ppu.vcount().into(),
            BG0CNT => self.ppu.bgcnt(0),
            BG1CNT => self.ppu.bgcnt(1),
            BG2CNT => self.ppu.bgcnt(2),
            BG3CNT => self.ppu.bgcnt(3),
            BG0HOFS => self.ppu.bghofs(0),
            BG1HOFS => self.ppu.bghofs(1),
            BG2HOFS => self.ppu.bghofs(2),
            BG3HOFS => self.ppu.bghofs(3),
            BG0VOFS => self.ppu.bgvofs(0),
            BG1VOFS => self.ppu.bgvofs(1),
            BG2VOFS => self.ppu.bgvofs(2),
            BG3VOFS => self.ppu.bgvofs(3),
            KEYINPUT => 0x03ff,
            IO_START..=0x0400_03ff => 0,
            _ => 0,
        }
    }

    fn write_byte(&mut self, addr: u32, value: u8) {
        let aligned = addr & !1;
        let mut current = self.read_halfword(aligned);
        if addr & 1 == 0 {
            current = (current & 0xff00) | u16::from(value);
        } else {
            current = (current & 0x00ff) | (u16::from(value) << 8);
        }
        self.write_halfword(aligned, current);
    }

    fn write_halfword(&mut self, addr: u32, value: u16) {
        match addr {
            DISPCNT => self.ppu.write_dispcnt(value),
            DISPSTAT => self.ppu.write_dispstat(value),
            BG0CNT => self.ppu.write_bgcnt(0, value),
            BG1CNT => self.ppu.write_bgcnt(1, value),
            BG2CNT => self.ppu.write_bgcnt(2, value),
            BG3CNT => self.ppu.write_bgcnt(3, value),
            BG0HOFS => self.ppu.write_bghofs(0, value),
            BG1HOFS => self.ppu.write_bghofs(1, value),
            BG2HOFS => self.ppu.write_bghofs(2, value),
            BG3HOFS => self.ppu.write_bghofs(3, value),
            BG0VOFS => self.ppu.write_bgvofs(0, value),
            BG1VOFS => self.ppu.write_bgvofs(1, value),
            BG2VOFS => self.ppu.write_bgvofs(2, value),
            BG3VOFS => self.ppu.write_bgvofs(3, value),
            IO_START..=0x0400_03ff => {}
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::ppu::{BG2_ENABLE, MODE_3};

    #[test]
    fn dispcnt_accepts_halfword_and_byte_access() {
        let mut memory = GbaMemory::new();
        let mut bus = Bus::new(&mut memory);

        bus.write(DISPCNT, AccessSize::Byte, MODE_3 as u32);
        bus.write(DISPCNT + 1, AccessSize::Byte, (BG2_ENABLE >> 8) as u32);

        assert_eq!(
            bus.read(DISPCNT, AccessSize::Halfword),
            u32::from(MODE_3 | BG2_ENABLE)
        );
    }

    #[test]
    fn word_reads_pack_adjacent_io_halfwords() {
        let mut memory = GbaMemory::new();
        let mut bus = Bus::new(&mut memory);

        bus.ppu_mut().step_scanline();

        assert_eq!(bus.read(DISPSTAT, AccessSize::Word) >> 16, 1);
    }
}

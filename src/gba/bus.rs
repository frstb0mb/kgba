use super::{
    memory::GbaMemory,
    memory_map::{
        BG0CNT, BG0HOFS, BG0VOFS, BG1CNT, BG1HOFS, BG1VOFS, BG2CNT, BG2HOFS, BG2PA, BG2PB, BG2PC,
        BG2PD, BG2VOFS, BG2X, BG2Y, BG3CNT, BG3HOFS, BG3VOFS, BLDALPHA, BLDCNT, BLDY, DISPCNT,
        DISPSTAT, IO_SIZE, IO_START, KEYINPUT, MOSAIC, VCOUNT, WIN0H, WIN0V, WIN1H, WIN1V, WININ,
        WINOUT,
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
    io: Box<[u8; IO_SIZE]>,
}

impl<'a> Bus<'a> {
    pub fn new(memory: &'a mut GbaMemory) -> Self {
        let mut io = Box::new([0; IO_SIZE]);
        let keyinput = (0x03ffu16).to_le_bytes();
        let keyinput_offset = (KEYINPUT - IO_START) as usize;
        io[keyinput_offset] = keyinput[0];
        io[keyinput_offset + 1] = keyinput[1];
        let bg2pa_offset = (BG2PA - IO_START) as usize;
        let bg2pd_offset = (BG2PD - IO_START) as usize;
        let identity = 0x0100u16.to_le_bytes();
        io[bg2pa_offset] = identity[0];
        io[bg2pa_offset + 1] = identity[1];
        io[bg2pd_offset] = identity[0];
        io[bg2pd_offset + 1] = identity[1];
        Self {
            memory,
            ppu: Ppu::new(),
            io,
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
            BG2PA => self.ppu.bgpa(2),
            BG2PB => self.ppu.bgpb(2),
            BG2PC => self.ppu.bgpc(2),
            BG2PD => self.ppu.bgpd(2),
            BG2X => self.ppu.bgx(2) as u16,
            addr if addr == BG2X + 2 => (self.ppu.bgx(2) >> 16) as u16,
            BG2Y => self.ppu.bgy(2) as u16,
            addr if addr == BG2Y + 2 => (self.ppu.bgy(2) >> 16) as u16,
            WIN0H => self.ppu.winh(0),
            WIN1H => self.ppu.winh(1),
            WIN0V => self.ppu.winv(0),
            WIN1V => self.ppu.winv(1),
            WININ => self.ppu.winin(),
            WINOUT => self.ppu.winout(),
            MOSAIC => self.ppu.mosaic(),
            BLDCNT => self.ppu.bldcnt(),
            BLDALPHA => self.ppu.bldalpha(),
            BLDY => self.ppu.bldy(),
            KEYINPUT => 0x03ff,
            IO_START..=0x0400_03ff => self.read_io_halfword(addr),
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
        if (IO_START..=0x0400_03ff).contains(&addr) {
            self.write_io_halfword(addr, value);
        }
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
            BG2PA => self.ppu.write_bgpa(2, value),
            BG2PB => self.ppu.write_bgpb(2, value),
            BG2PC => self.ppu.write_bgpc(2, value),
            BG2PD => self.ppu.write_bgpd(2, value),
            BG2X => self.ppu.write_bgx_halfword(2, 0, value),
            addr if addr == BG2X + 2 => self.ppu.write_bgx_halfword(2, 1, value),
            BG2Y => self.ppu.write_bgy_halfword(2, 0, value),
            addr if addr == BG2Y + 2 => self.ppu.write_bgy_halfword(2, 1, value),
            WIN0H => self.ppu.write_winh(0, value),
            WIN1H => self.ppu.write_winh(1, value),
            WIN0V => self.ppu.write_winv(0, value),
            WIN1V => self.ppu.write_winv(1, value),
            WININ => self.ppu.write_winin(value),
            WINOUT => self.ppu.write_winout(value),
            MOSAIC => self.ppu.write_mosaic(value),
            BLDCNT => self.ppu.write_bldcnt(value),
            BLDALPHA => self.ppu.write_bldalpha(value),
            BLDY => self.ppu.write_bldy(value),
            IO_START..=0x0400_03ff => {}
            _ => {}
        }
    }

    fn read_io_halfword(&self, addr: u32) -> u16 {
        let offset = (addr - IO_START) as usize;
        u16::from_le_bytes([self.io[offset], self.io[offset + 1]])
    }

    fn write_io_halfword(&mut self, addr: u32, value: u16) {
        let offset = (addr - IO_START) as usize;
        let bytes = value.to_le_bytes();
        self.io[offset] = bytes[0];
        self.io[offset + 1] = bytes[1];
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

    #[test]
    fn window_registers_accept_halfword_and_byte_access() {
        let mut memory = GbaMemory::new();
        let mut bus = Bus::new(&mut memory);

        bus.write(WIN0H, AccessSize::Halfword, 0x1428);
        bus.write(WININ, AccessSize::Byte, 0x02);
        bus.write(WINOUT + 1, AccessSize::Byte, 0x20);

        assert_eq!(bus.read(WIN0H, AccessSize::Halfword), 0x1428);
        assert_eq!(bus.read(WININ, AccessSize::Halfword), 0x02);
        assert_eq!(bus.read(WINOUT, AccessSize::Halfword), 0x2000);
    }

    #[test]
    fn blend_registers_accept_halfword_and_byte_access() {
        let mut memory = GbaMemory::new();
        let mut bus = Bus::new(&mut memory);

        bus.write(BLDCNT, AccessSize::Halfword, 0x3fff);
        bus.write(BLDALPHA, AccessSize::Byte, 0x08);
        bus.write(BLDALPHA + 1, AccessSize::Byte, 0x08);
        bus.write(BLDY, AccessSize::Halfword, 0x0010);

        assert_eq!(bus.read(BLDCNT, AccessSize::Halfword), 0x3fff);
        assert_eq!(bus.read(BLDALPHA, AccessSize::Halfword), 0x0808);
        assert_eq!(bus.read(BLDY, AccessSize::Halfword), 0x0010);
    }

    #[test]
    fn bg2_affine_registers_accept_halfword_and_word_access() {
        let mut memory = GbaMemory::new();
        let mut bus = Bus::new(&mut memory);

        assert_eq!(bus.read(BG2PA, AccessSize::Halfword), 0x0100);
        assert_eq!(bus.read(BG2PD, AccessSize::Halfword), 0x0100);

        bus.write(BG2PA, AccessSize::Halfword, 0x0200);
        bus.write(BG2X, AccessSize::Word, 0x0001_8000);
        bus.write(BG2Y, AccessSize::Word, 0x0fff_8000);

        assert_eq!(bus.read(BG2PA, AccessSize::Halfword), 0x0200);
        assert_eq!(bus.read(BG2X, AccessSize::Word), 0x0001_8000);
        assert_eq!(bus.read(BG2Y, AccessSize::Word), 0x0fff_8000);
    }
}

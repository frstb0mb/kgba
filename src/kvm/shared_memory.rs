use std::sync::Mutex;

use crate::gba::{
    memory_map::{
        BG0CNT, BG0HOFS, BG0VOFS, BG1CNT, BG1HOFS, BG1VOFS, BG2CNT, BG2HOFS, BG2VOFS, BG3CNT,
        BG3HOFS, BG3VOFS, DISPCNT, DMA0CNT, DMA0SAD, EWRAM_SIZE, EWRAM_START, GAME_PAK_ROM_START,
        IO_START, IWRAM_SIZE, IWRAM_START, KEYINPUT, OAM_SIZE, OAM_START, PALETTE_SIZE,
        PALETTE_START, VCOUNT, VRAM_SIZE, VRAM_START,
    },
    ppu::{FrameBuffer, Ppu},
};

use super::{memory::MemoryRegion, timers::Timers, trace::trace_timer_register_write};

#[derive(Debug)]
pub struct KvmSharedMemory {
    pub(super) ewram: MemoryRegion,
    pub(super) iwram: MemoryRegion,
    pub(super) io: MemoryRegion,
    pub(super) palette: MemoryRegion,
    pub(super) vram: MemoryRegion,
    pub(super) oam: MemoryRegion,
    rom: Box<[u8]>,
    pub(super) timers: Mutex<Timers>,
}

unsafe impl Send for KvmSharedMemory {}
unsafe impl Sync for KvmSharedMemory {}

impl KvmSharedMemory {
    pub fn new(
        ewram: MemoryRegion,
        iwram: MemoryRegion,
        io: MemoryRegion,
        palette: MemoryRegion,
        vram: MemoryRegion,
        oam: MemoryRegion,
        rom: &[u8],
    ) -> Self {
        Self {
            ewram,
            iwram,
            io,
            palette,
            vram,
            oam,
            rom: rom.to_vec().into_boxed_slice(),
            timers: Mutex::new(Timers::new()),
        }
    }

    pub fn set_vcount(&self, value: u16) {
        self.write_io_u16(VCOUNT, value);
    }

    pub fn tick_scanline(&self) {
        self.advance_timers(1_232);
    }

    pub fn set_keyinput(&self, value: u16) {
        self.write_io_u16(KEYINPUT, value);
    }

    pub fn render_frame(&self) -> FrameBuffer {
        let mut ppu = Ppu::new();
        ppu.write_dispcnt(self.read_io_u16(DISPCNT));
        ppu.write_bgcnt(0, self.read_io_u16(BG0CNT));
        ppu.write_bgcnt(1, self.read_io_u16(BG1CNT));
        ppu.write_bgcnt(2, self.read_io_u16(BG2CNT));
        ppu.write_bgcnt(3, self.read_io_u16(BG3CNT));
        ppu.write_bghofs(0, self.read_io_u16(BG0HOFS));
        ppu.write_bghofs(1, self.read_io_u16(BG1HOFS));
        ppu.write_bghofs(2, self.read_io_u16(BG2HOFS));
        ppu.write_bghofs(3, self.read_io_u16(BG3HOFS));
        ppu.write_bgvofs(0, self.read_io_u16(BG0VOFS));
        ppu.write_bgvofs(1, self.read_io_u16(BG1VOFS));
        ppu.write_bgvofs(2, self.read_io_u16(BG2VOFS));
        ppu.write_bgvofs(3, self.read_io_u16(BG3VOFS));
        ppu.render_frame(
            self.palette.as_slice(),
            self.vram.as_slice(),
            self.oam.as_slice(),
        )
    }

    pub(super) fn read_io_u16(&self, addr: u32) -> u16 {
        let offset = (addr - IO_START) as usize;
        let io = self.io.as_slice();
        u16::from_le_bytes([io[offset], io[offset + 1]])
    }

    pub(super) fn write_io_u16(&self, addr: u32, value: u16) {
        let offset = (addr - IO_START) as usize;
        let bytes = value.to_le_bytes();
        let io = self.io.as_mut_slice();
        io[offset] = bytes[0];
        io[offset + 1] = bytes[1];
    }

    pub(super) fn mirror_io_write(&self, addr: u32, len: u32, data: &[u8; 8]) {
        let offset = (addr - IO_START) as usize;
        let len = len as usize;
        self.io.as_mut_slice()[offset..offset + len].copy_from_slice(&data[..len]);
    }

    pub(super) fn run_immediate_dma_for_io_write(&self, addr: u32, len: u32) {
        for register in (addr..addr + len).step_by(2) {
            self.run_immediate_dma_if_enabled(register);
        }
    }

    pub(super) fn copy_io_read(&self, addr: u32, len: u32, data: &mut [u8; 8]) {
        let offset = (addr - IO_START) as usize;
        let len = len as usize;
        data.fill(0);
        data[..len].copy_from_slice(&self.io.as_slice()[offset..offset + len]);
    }

    pub(super) fn write_timer_registers_from_io(&self, addr: u32, len: u32) {
        let start = addr.max(IO_START + 0x0100);
        let end = (addr + len).min(IO_START + 0x0110);
        let start = start & !1;
        let end = (end + 1) & !1;

        for register in (start..end).step_by(2) {
            if !(IO_START + 0x0100..=IO_START + 0x010e).contains(&register) {
                continue;
            }
            let value = self.read_io_u16(register);
            trace_timer_register_write(register, value);
            self.timers
                .lock()
                .expect("timer lock poisoned")
                .write_register(register, value, &self.io);
        }
    }

    fn advance_timers(&self, cycles: u32) {
        self.timers
            .lock()
            .expect("timer lock poisoned")
            .advance(cycles, &self.io);
    }

    fn run_immediate_dma_if_enabled(&self, addr: u32) {
        let Some(channel) = dma_channel_for_cnt_high(addr) else {
            return;
        };

        let base = DMA0SAD + channel as u32 * 12;
        let control = self.read_io_u16(base + 10);
        if control & DMA_ENABLE == 0 || control & DMA_TIMING_MASK != 0 {
            return;
        }

        let source = self.read_io_u32(base);
        let destination = self.read_io_u32(base + 4);
        let count = self.read_io_u16(base + 8);
        self.run_dma(channel, source, destination, count, control);

        if control & DMA_REPEAT == 0 {
            self.write_io_u16(base + 10, control & !DMA_ENABLE);
        }
    }

    fn run_dma(
        &self,
        channel: usize,
        mut source: u32,
        mut destination: u32,
        count: u16,
        control: u16,
    ) {
        let unit_size = if control & DMA_32BIT != 0 { 4 } else { 2 };
        let count = if count == 0 {
            if channel == 3 { 0x10000 } else { 0x4000 }
        } else {
            u32::from(count)
        };
        let source_step = dma_addr_step((control >> 7) & 0x3, unit_size);
        let destination_step = dma_addr_step((control >> 5) & 0x3, unit_size);

        for _ in 0..count {
            if unit_size == 4 {
                let low = self.dma_read_halfword(source);
                let high = self.dma_read_halfword(source.wrapping_add(2));
                self.dma_write_halfword(destination, low);
                self.dma_write_halfword(destination.wrapping_add(2), high);
            } else {
                let value = self.dma_read_halfword(source);
                self.dma_write_halfword(destination, value);
            }
            source = source.wrapping_add_signed(source_step);
            destination = destination.wrapping_add_signed(destination_step);
        }
    }

    fn read_io_u32(&self, addr: u32) -> u32 {
        u32::from(self.read_io_u16(addr)) | (u32::from(self.read_io_u16(addr + 2)) << 16)
    }

    fn dma_read_halfword(&self, addr: u32) -> u16 {
        if (GAME_PAK_ROM_START..GAME_PAK_ROM_START + self.rom.len() as u32).contains(&addr) {
            let offset = (addr - GAME_PAK_ROM_START) as usize;
            if offset + 1 < self.rom.len() {
                return u16::from_le_bytes([self.rom[offset], self.rom[offset + 1]]);
            }
            return 0xffff;
        }
        dma_memory_region(self, addr).map_or(0, |(memory, offset)| {
            u16::from_le_bytes([memory[offset], memory[offset + 1]])
        })
    }

    fn dma_write_halfword(&self, addr: u32, value: u16) {
        let Some((memory, offset)) = dma_memory_region_mut(self, addr) else {
            return;
        };
        let bytes = value.to_le_bytes();
        memory[offset] = bytes[0];
        memory[offset + 1] = bytes[1];
    }
}

const DMA_ENABLE: u16 = 1 << 15;
const DMA_REPEAT: u16 = 1 << 9;
const DMA_32BIT: u16 = 1 << 10;
const DMA_TIMING_MASK: u16 = 0x3000;

fn dma_channel_for_cnt_high(addr: u32) -> Option<usize> {
    if addr < DMA0CNT + 2 || addr > DMA0CNT + 2 + 3 * 12 {
        return None;
    }
    let offset = addr - (DMA0CNT + 2);
    if offset % 12 == 0 {
        Some((offset / 12) as usize)
    } else {
        None
    }
}

fn dma_addr_step(mode: u16, unit_size: i32) -> i32 {
    match mode {
        1 => -unit_size,
        2 => 0,
        _ => unit_size,
    }
}

fn dma_memory_region(shared: &KvmSharedMemory, addr: u32) -> Option<(&[u8], usize)> {
    if (EWRAM_START..EWRAM_START + EWRAM_SIZE as u32).contains(&addr) {
        Some((shared.ewram.as_slice(), (addr - EWRAM_START) as usize))
    } else if (IWRAM_START..IWRAM_START + IWRAM_SIZE as u32).contains(&addr) {
        Some((shared.iwram.as_slice(), (addr - IWRAM_START) as usize))
    } else if (PALETTE_START..PALETTE_START + PALETTE_SIZE as u32).contains(&addr) {
        Some((shared.palette.as_slice(), (addr - PALETTE_START) as usize))
    } else if (VRAM_START..VRAM_START + VRAM_SIZE as u32).contains(&addr) {
        Some((shared.vram.as_slice(), (addr - VRAM_START) as usize))
    } else if (OAM_START..OAM_START + OAM_SIZE as u32).contains(&addr) {
        Some((shared.oam.as_slice(), (addr - OAM_START) as usize))
    } else {
        None
    }
}

fn dma_memory_region_mut(shared: &KvmSharedMemory, addr: u32) -> Option<(&mut [u8], usize)> {
    if (EWRAM_START..EWRAM_START + EWRAM_SIZE as u32).contains(&addr) {
        Some((shared.ewram.as_mut_slice(), (addr - EWRAM_START) as usize))
    } else if (IWRAM_START..IWRAM_START + IWRAM_SIZE as u32).contains(&addr) {
        Some((shared.iwram.as_mut_slice(), (addr - IWRAM_START) as usize))
    } else if (PALETTE_START..PALETTE_START + PALETTE_SIZE as u32).contains(&addr) {
        Some((
            shared.palette.as_mut_slice(),
            (addr - PALETTE_START) as usize,
        ))
    } else if (VRAM_START..VRAM_START + VRAM_SIZE as u32).contains(&addr) {
        Some((shared.vram.as_mut_slice(), (addr - VRAM_START) as usize))
    } else if (OAM_START..OAM_START + OAM_SIZE as u32).contains(&addr) {
        Some((shared.oam.as_mut_slice(), (addr - OAM_START) as usize))
    } else {
        None
    }
}

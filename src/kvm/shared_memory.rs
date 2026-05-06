use std::sync::Mutex;

use crate::gba::{
    memory_map::{DISPCNT, IO_START, VCOUNT},
    ppu::{FrameBuffer, Ppu},
};

use super::{memory::MemoryRegion, timers::Timers, trace::trace_timer_register_write};

#[derive(Debug)]
pub struct KvmSharedMemory {
    pub(super) io: MemoryRegion,
    pub(super) palette: MemoryRegion,
    pub(super) vram: MemoryRegion,
    pub(super) oam: MemoryRegion,
    pub(super) timers: Mutex<Timers>,
}

unsafe impl Send for KvmSharedMemory {}
unsafe impl Sync for KvmSharedMemory {}

impl KvmSharedMemory {
    pub fn new(
        io: MemoryRegion,
        palette: MemoryRegion,
        vram: MemoryRegion,
        oam: MemoryRegion,
    ) -> Self {
        Self {
            io,
            palette,
            vram,
            oam,
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
        self.write_io_u16(IO_START + 0x0130, value);
    }

    pub fn render_frame(&self) -> FrameBuffer {
        let mut ppu = Ppu::new();
        ppu.write_dispcnt(self.read_io_u16(DISPCNT));
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
}

use std::{
    os::fd::RawFd,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::gba::{
    memory_map::{
        BG0CNT, BG0HOFS, BG0VOFS, BG1CNT, BG1HOFS, BG1VOFS, BG2CNT, BG2HOFS, BG2VOFS, BG3CNT,
        BG3HOFS, BG3VOFS, DISPCNT, DISPSTAT, DMA0CNT, DMA0SAD, EWRAM_SIZE, EWRAM_START,
        GAME_PAK_ROM_START, IE, IF, IME, IO_START, IWRAM_SIZE, IWRAM_START, KEYINPUT, MOSAIC,
        OAM_SIZE, OAM_START, PALETTE_SIZE, PALETTE_START, VCOUNT, VRAM_SIZE, VRAM_START, WIN0H,
        WIN0V, WIN1H, WIN1V, WININ, WINOUT,
    },
    ppu::{FrameBuffer, Ppu},
};

use super::{
    memory::MemoryRegion,
    sys,
    timers::Timers,
    trace::{
        trace_input_io_write, trace_input_irq_line, trace_input_keyinput, trace_input_vblank,
        trace_timer_register_write,
    },
    util::last_os_error,
};

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
    interrupt_line: InterruptLine,
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
        vm_fd: RawFd,
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
            interrupt_line: InterruptLine::new(vm_fd),
        }
    }

    pub fn set_vcount(&self, value: u16) {
        let old_dispstat = self.read_io_u16(DISPSTAT);
        self.write_io_u16(VCOUNT, value);
        let mut dispstat = old_dispstat & !DISPSTAT_VBLANK;
        if value >= 160 {
            dispstat |= DISPSTAT_VBLANK;
        }
        self.write_io_u16(DISPSTAT, dispstat);

        if old_dispstat & DISPSTAT_VBLANK == 0
            && dispstat & DISPSTAT_VBLANK != 0
            && dispstat & DISPSTAT_VBLANK_IRQ_ENABLE != 0
        {
            trace_input_vblank(
                value,
                self.read_io_u16(IE),
                self.read_io_u16(IF),
                self.read_io_u16(IME),
            );
            self.request_interrupt(IRQ_VBLANK);
        }
    }

    pub fn tick_scanline(&self) {
        self.advance_timers(1_232);
    }

    pub fn set_keyinput(&self, value: u16) {
        let old = self.read_io_u16(KEYINPUT);
        self.write_io_u16(KEYINPUT, value);
        if old != value {
            trace_input_keyinput(value);
        }
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
        ppu.write_winh(0, self.read_io_u16(WIN0H));
        ppu.write_winh(1, self.read_io_u16(WIN1H));
        ppu.write_winv(0, self.read_io_u16(WIN0V));
        ppu.write_winv(1, self.read_io_u16(WIN1V));
        ppu.write_winin(self.read_io_u16(WININ));
        ppu.write_winout(self.read_io_u16(WINOUT));
        ppu.write_mosaic(self.read_io_u16(MOSAIC));
        ppu.render_frame(
            self.palette.as_slice(),
            self.vram.as_slice(),
            self.oam.as_slice(),
        )
    }

    pub fn debug_video_state(&self) -> KvmVideoDebugState {
        KvmVideoDebugState {
            dispcnt: self.read_io_u16(DISPCNT),
            bg2cnt: self.read_io_u16(BG2CNT),
            mosaic: self.read_io_u16(MOSAIC),
            dma3cnt: self.read_io_u16(DMA0CNT + 3 * 12 + 2),
            vram0: u16::from_le_bytes([self.vram.as_slice()[0], self.vram.as_slice()[1]]),
        }
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
        let old_if = self.read_io_u16(IF);
        let old_dispstat = self.read_io_u16(DISPSTAT);
        let if_write = io_register_write(IF, addr, len, data);
        let dispstat_write = io_register_write(DISPSTAT, addr, len, data);
        let offset = (addr - IO_START) as usize;
        let len = len as usize;
        self.io.as_mut_slice()[offset..offset + len].copy_from_slice(&data[..len]);
        if let Some(write) = if_write {
            self.write_io_u16(IF, old_if & !write.value);
            trace_input_io_write(
                IF,
                self.read_io_u16(IF),
                self.read_io_u16(KEYINPUT),
                self.read_io_u16(VCOUNT),
            );
        }
        if let Some(write) = dispstat_write {
            let writable = write.value & DISPSTAT_WRITABLE_MASK;
            let preserved = old_dispstat & !write.mask;
            let status = old_dispstat & DISPSTAT_STATUS_MASK;
            self.write_io_u16(DISPSTAT, status | preserved | writable);
            trace_input_io_write(
                DISPSTAT,
                self.read_io_u16(DISPSTAT),
                self.read_io_u16(KEYINPUT),
                self.read_io_u16(VCOUNT),
            );
        }
        for register in [IE, IME, MOSAIC] {
            if let Some(write) = io_register_write(register, addr, len as u32, data) {
                trace_input_io_write(
                    register,
                    write.value,
                    self.read_io_u16(KEYINPUT),
                    self.read_io_u16(VCOUNT),
                );
            }
        }
        if let Some(write) = io_register_write(KEYINPUT, addr, len as u32, data) {
            trace_input_io_write(
                KEYINPUT,
                write.value,
                self.read_io_u16(KEYINPUT),
                self.read_io_u16(VCOUNT),
            );
        }
        self.update_interrupt_line();
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

    fn request_interrupt(&self, interrupt: u16) {
        self.write_io_u16(IF, self.read_io_u16(IF) | interrupt);
        self.update_interrupt_line();
    }

    fn update_interrupt_line(&self) {
        let enabled = self.read_io_u16(IE);
        let requested = self.read_io_u16(IF);
        let master_enabled = self.read_io_u16(IME) & 1 != 0;
        let asserted = master_enabled && enabled & requested != 0;
        self.interrupt_line.set(asserted);
        trace_input_irq_line(asserted, enabled, requested, self.read_io_u16(IME));
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

#[derive(Clone, Copy, Debug)]
pub struct KvmVideoDebugState {
    pub dispcnt: u16,
    pub bg2cnt: u16,
    pub mosaic: u16,
    pub dma3cnt: u16,
    pub vram0: u16,
}

const DISPSTAT_VBLANK: u16 = 1 << 0;
const DISPSTAT_STATUS_MASK: u16 = 0x0007;
const DISPSTAT_WRITABLE_MASK: u16 = 0xff38;
const DISPSTAT_VBLANK_IRQ_ENABLE: u16 = 1 << 3;
const IRQ_VBLANK: u16 = 1 << 0;
const DMA_ENABLE: u16 = 1 << 15;
const DMA_REPEAT: u16 = 1 << 9;
const DMA_32BIT: u16 = 1 << 10;
const DMA_TIMING_MASK: u16 = 0x3000;

#[derive(Debug)]
struct InterruptLine {
    vm_fd: RawFd,
    asserted: AtomicBool,
}

impl InterruptLine {
    fn new(vm_fd: RawFd) -> Self {
        Self {
            vm_fd,
            asserted: AtomicBool::new(false),
        }
    }

    fn set(&self, asserted: bool) {
        if self.asserted.swap(asserted, Ordering::Relaxed) == asserted {
            return;
        }

        let mut level = sys::KvmIrqLevel {
            irq: KVM_ARM_CPU_IRQ,
            level: u32::from(asserted),
        };
        let ret = unsafe {
            sys::ioctl_ptr(
                self.vm_fd,
                sys::KVM_IRQ_LINE,
                (&mut level as *mut sys::KvmIrqLevel).cast(),
            )
        };
        if ret != 0 {
            eprintln!("{}", last_os_error("KVM_IRQ_LINE"));
        }
    }
}

const KVM_ARM_CPU_IRQ: u32 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IoRegisterWrite {
    mask: u16,
    value: u16,
}

fn io_register_write(
    register: u32,
    addr: u32,
    len: u32,
    data: &[u8; 8],
) -> Option<IoRegisterWrite> {
    let mut mask = 0;
    let mut value = 0;
    for index in 0..len as usize {
        let byte_addr = addr + index as u32;
        if !(register..register + 2).contains(&byte_addr) {
            continue;
        }
        let shift = (byte_addr - register) * 8;
        mask |= 0xff << shift;
        value |= u16::from(data[index]) << shift;
    }
    if mask == 0 {
        None
    } else {
        Some(IoRegisterWrite { mask, value })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GAME_PAK_ROM_START, IoRegisterWrite, KvmSharedMemory, MOSAIC, PALETTE_SIZE, VRAM_SIZE,
        io_register_write,
    };
    use crate::{
        gba::memory_map::{DMA3CNT, DMA3DAD, DMA3SAD, EWRAM_SIZE, IWRAM_SIZE, OAM_SIZE},
        kvm::memory::MemoryRegion,
    };

    #[test]
    fn io_register_write_extracts_partial_halfword_write() {
        let data = [0x34, 0x12, 0, 0, 0, 0, 0, 0];

        assert_eq!(
            io_register_write(0x0400_0202, 0x0400_0202, 2, &data),
            Some(IoRegisterWrite {
                mask: 0xffff,
                value: 0x1234
            })
        );
        assert_eq!(
            io_register_write(0x0400_0202, 0x0400_0203, 1, &data),
            Some(IoRegisterWrite {
                mask: 0xff00,
                value: 0x3400
            })
        );
        assert_eq!(io_register_write(0x0400_0202, 0x0400_0204, 2, &data), None);
    }

    #[test]
    fn immediate_dma3_copies_mode3_bitmap_from_rom_to_vram() {
        let count = 240 * 160;
        let source_offset = 0x31c;
        let mut rom = vec![0; source_offset + count * 2];
        rom[source_offset..source_offset + 4].copy_from_slice(&[0x5f, 0x4a, 0x34, 0x12]);
        let last_offset = source_offset + (count - 1) * 2;
        rom[last_offset..last_offset + 2].copy_from_slice(&[0x1f, 0x00]);

        let shared = test_shared_memory(&rom);
        shared.write_io_u16(DMA3SAD, (GAME_PAK_ROM_START + source_offset as u32) as u16);
        shared.write_io_u16(
            DMA3SAD + 2,
            ((GAME_PAK_ROM_START + source_offset as u32) >> 16) as u16,
        );
        shared.write_io_u16(DMA3DAD, 0);
        shared.write_io_u16(DMA3DAD + 2, 0x0600);
        shared.write_io_u16(DMA3CNT, count as u16);
        shared.write_io_u16(DMA3CNT + 2, 0x8000);

        shared.run_immediate_dma_for_io_write(DMA3CNT, 4);

        assert_eq!(&shared.vram.as_slice()[0..4], &[0x5f, 0x4a, 0x34, 0x12]);
        assert_eq!(
            &shared.vram.as_slice()[(count - 1) * 2..count * 2],
            &[0x1f, 0x00]
        );
        assert_eq!(shared.read_io_u16(DMA3CNT + 2) & 0x8000, 0);
        assert_eq!(shared.read_io_u16(MOSAIC), 0);
    }

    fn test_shared_memory(rom: &[u8]) -> KvmSharedMemory {
        KvmSharedMemory::new(
            MemoryRegion::anonymous(EWRAM_SIZE).expect("ewram"),
            MemoryRegion::anonymous(IWRAM_SIZE).expect("iwram"),
            MemoryRegion::anonymous(0x1000).expect("io"),
            MemoryRegion::anonymous(PALETTE_SIZE).expect("palette"),
            MemoryRegion::anonymous(VRAM_SIZE).expect("vram"),
            MemoryRegion::anonymous(OAM_SIZE).expect("oam"),
            rom,
            -1,
        )
    }
}

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

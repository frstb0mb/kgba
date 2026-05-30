use std::{
    collections::VecDeque,
    os::fd::RawFd,
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::gba::{
    memory_map::{
        BG0CNT, BG0HOFS, BG0VOFS, BG1CNT, BG1HOFS, BG1VOFS, BG2CNT, BG2HOFS, BG2PA, BG2PB, BG2PC,
        BG2PD, BG2VOFS, BG2X, BG2Y, BG3CNT, BG3HOFS, BG3VOFS, BLDALPHA, BLDCNT, BLDY, DISPCNT,
        DISPSTAT, DMA0CNT, DMA0SAD, EWRAM_SIZE, EWRAM_START, FIFO_A, FIFO_B, GAME_PAK_ROM_START,
        IE, IF, IME, IO_START, IWRAM_SIZE, IWRAM_START, KEYINPUT, MOSAIC, OAM_SIZE, OAM_START,
        PALETTE_SIZE, PALETTE_START, SOUNDCNT_H, VCOUNT, VRAM_SIZE, VRAM_START, WIN0H, WIN0V,
        WIN1H, WIN1V, WININ, WINOUT,
    },
    ppu::{
        BG0_ENABLE, BG1_ENABLE, BG2_ENABLE, BG3_ENABLE, DISPCNT_MODE_MASK, FrameBuffer, HEIGHT,
        MODE_0, OBJ_ENABLE, OBJ_WIN_ENABLE, Ppu, WIDTH, WIN0_ENABLE, WIN1_ENABLE,
    },
};

use super::{
    bootstrap::{FAST_CTRL_ADDR, FAST_MEM_START, SHADOW_IO_ADDR},
    memory::MemoryRegion,
    sys,
    timers::Timers,
    trace::{
        trace_fast_hblank, trace_hblank_pending, trace_hblank_wait, trace_hblank_wait_timeout,
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
    pub(super) fast: MemoryRegion,
    rom: Box<[u8]>,
    pub(super) timers: Mutex<Timers>,
    audio: Mutex<AudioState>,
    bg_offset_scanline: Mutex<BgOffsetRaster>,
    bghofs_scanline_active: [AtomicBool; 4],
    bgvofs_scanline_active: [AtomicBool; 4],
    frame_buffers: Mutex<FrameBuffers>,
    completed_frame_seq: AtomicU64,
    hblank_sync: HblankSync,
    hblank_latch_vcount: AtomicU16,
    hblank_latch_seq: AtomicU64,
    hblank_pending_count: AtomicU64,
    perf: VideoPerfCounters,
    interrupt_line: InterruptLine,
}

unsafe impl Send for KvmSharedMemory {}
unsafe impl Sync for KvmSharedMemory {}

#[derive(Debug)]
struct BgOffsetRaster {
    hofs: [[u16; HEIGHT]; 4],
    completed_hofs: [[u16; HEIGHT]; 4],
    vofs: [[u16; HEIGHT]; 4],
    completed_vofs: [[u16; HEIGHT]; 4],
}

#[derive(Debug)]
struct FrameBuffers {
    work: FrameBuffer,
    completed: Option<Arc<FrameBuffer>>,
    spare: Option<FrameBuffer>,
    seq: u64,
}

#[derive(Debug, Default)]
struct HblankSync {
    requested_seq: AtomicU64,
    completed_seq: AtomicU64,
}

#[derive(Debug)]
struct AudioState {
    fifo_a: VecDeque<i8>,
    fifo_b: VecDeque<i8>,
    pcm: VecDeque<i16>,
}

#[derive(Debug, Default)]
struct VideoPerfCounters {
    frames: AtomicU64,
    render_scanline_us: AtomicU64,
    hblank_wait_us: AtomicU64,
    hblank_wait_max_us: AtomicU64,
    hblank_wait_timeouts: AtomicU64,
    fast_hblank_us: AtomicU64,
    fast_hblank_count: AtomicU64,
    fast_hblank_shared_count: AtomicU64,
    fast_hblank_mmio_count: AtomicU64,
    kvm_mmio_exits: AtomicU64,
    kvm_mmio_fast_exit: AtomicU64,
    kvm_mmio_io_reads: AtomicU64,
    kvm_mmio_io_writes: AtomicU64,
    kvm_mmio_io_if: AtomicU64,
    kvm_mmio_io_ime: AtomicU64,
    kvm_mmio_io_bg_hofs: AtomicU64,
    kvm_mmio_io_bg_vofs: AtomicU64,
    kvm_mmio_io_other: AtomicU64,
    sdl_present_us: AtomicU64,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct VideoPerfSnapshot {
    pub frames: u64,
    pub render_scanline_us: u64,
    pub hblank_wait_us: u64,
    pub hblank_wait_max_us: u64,
    pub hblank_wait_timeouts: u64,
    pub fast_hblank_us: u64,
    pub fast_hblank_count: u64,
    pub fast_hblank_shared_count: u64,
    pub fast_hblank_mmio_count: u64,
    pub kvm_mmio_exits: u64,
    pub kvm_mmio_fast_exit: u64,
    pub kvm_mmio_io_reads: u64,
    pub kvm_mmio_io_writes: u64,
    pub kvm_mmio_io_if: u64,
    pub kvm_mmio_io_ime: u64,
    pub kvm_mmio_io_bg_hofs: u64,
    pub kvm_mmio_io_bg_vofs: u64,
    pub kvm_mmio_io_other: u64,
    pub sdl_present_us: u64,
}

#[derive(Debug, Clone)]
pub struct FrameSnapshot {
    pub seq: u64,
    pub pixels: Arc<FrameBuffer>,
}

impl Default for FrameBuffers {
    fn default() -> Self {
        Self {
            work: vec![0; WIDTH * HEIGHT],
            completed: Some(Arc::new(vec![0; WIDTH * HEIGHT])),
            spare: Some(vec![0; WIDTH * HEIGHT]),
            seq: 0,
        }
    }
}

impl Default for BgOffsetRaster {
    fn default() -> Self {
        Self {
            hofs: [[0; HEIGHT]; 4],
            completed_hofs: [[0; HEIGHT]; 4],
            vofs: [[0; HEIGHT]; 4],
            completed_vofs: [[0; HEIGHT]; 4],
        }
    }
}

impl Default for AudioState {
    fn default() -> Self {
        Self {
            fifo_a: VecDeque::with_capacity(AUDIO_FIFO_CAPACITY),
            fifo_b: VecDeque::with_capacity(AUDIO_FIFO_CAPACITY),
            pcm: VecDeque::with_capacity(AUDIO_PCM_CAPACITY),
        }
    }
}

impl KvmSharedMemory {
    pub fn new(
        ewram: MemoryRegion,
        iwram: MemoryRegion,
        io: MemoryRegion,
        palette: MemoryRegion,
        vram: MemoryRegion,
        oam: MemoryRegion,
        fast: MemoryRegion,
        rom: &[u8],
        vm_fd: RawFd,
    ) -> Self {
        let shared = Self {
            ewram,
            iwram,
            io,
            palette,
            vram,
            oam,
            fast,
            rom: rom.to_vec().into_boxed_slice(),
            timers: Mutex::new(Timers::new()),
            audio: Mutex::new(AudioState::default()),
            bg_offset_scanline: Mutex::new(BgOffsetRaster::default()),
            bghofs_scanline_active: std::array::from_fn(|_| AtomicBool::new(false)),
            bgvofs_scanline_active: std::array::from_fn(|_| AtomicBool::new(false)),
            frame_buffers: Mutex::new(FrameBuffers::default()),
            completed_frame_seq: AtomicU64::new(0),
            hblank_sync: HblankSync::default(),
            hblank_latch_vcount: AtomicU16::new(0xffff),
            hblank_latch_seq: AtomicU64::new(0),
            hblank_pending_count: AtomicU64::new(0),
            perf: VideoPerfCounters::default(),
            interrupt_line: InterruptLine::new(vm_fd),
        };
        shared.write_io_u16(BG2PA, 0x0100);
        shared.write_io_u16(BG2PD, 0x0100);
        shared
    }

    pub fn set_vcount(&self, value: u16) {
        let old_dispstat = self.read_io_u16(DISPSTAT);
        self.write_io_u16(VCOUNT, value);
        let mut dispstat = old_dispstat & !(DISPSTAT_VBLANK | DISPSTAT_HBLANK);
        if value >= 160 {
            dispstat |= DISPSTAT_VBLANK;
        }
        self.write_io_u16(DISPSTAT, dispstat);
        if value == 160 {
            let mut bg_offset = self
                .bg_offset_scanline
                .lock()
                .expect("bg offset scanline lock poisoned");
            bg_offset.completed_hofs = bg_offset.hofs;
            bg_offset.completed_vofs = bg_offset.vofs;
        }
        if usize::from(value) < HEIGHT {
            let mut bg_offset = self
                .bg_offset_scanline
                .lock()
                .expect("bg offset scanline lock poisoned");
            let y = usize::from(value);
            for (bg, register) in [BG0HOFS, BG1HOFS, BG2HOFS, BG3HOFS].into_iter().enumerate() {
                if !self.bghofs_scanline_active[bg].load(Ordering::Relaxed) {
                    bg_offset.hofs[bg][y] = self.read_io_u16(register);
                }
            }
            for (bg, register) in [BG0VOFS, BG1VOFS, BG2VOFS, BG3VOFS].into_iter().enumerate() {
                if !self.bgvofs_scanline_active[bg].load(Ordering::Relaxed) {
                    bg_offset.vofs[bg][y] = self.read_io_u16(register);
                }
            }
        }

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
            if self.read_io_u16(IE) & IRQ_VBLANK != 0 && self.read_io_u16(IME) & 1 != 0 {
                self.interrupt_line.pulse();
            }
        }
    }

    pub fn enter_hblank(&self) -> Option<u64> {
        let old_dispstat = self.read_io_u16(DISPSTAT);
        let dispstat = old_dispstat | DISPSTAT_HBLANK;
        self.write_io_u16(DISPSTAT, dispstat);
        if old_dispstat & DISPSTAT_HBLANK != 0 {
            return None;
        }
        let vcount = self.read_io_u16(VCOUNT);
        if dispstat & DISPSTAT_HBLANK_IRQ_ENABLE == 0
            || self.read_io_u16(DISPCNT) == 0
            || !(vcount < 160 || vcount == 227)
        {
            return None;
        }
        if self.read_io_u16(IF) & IRQ_HBLANK != 0 {
            if self.read_io_u16(IE) & IRQ_HBLANK != 0 && self.read_io_u16(IME) & 1 != 0 {
                self.hblank_pending_count.fetch_add(1, Ordering::Relaxed);
                trace_hblank_pending(
                    vcount,
                    self.read_io_u16(IE),
                    self.read_io_u16(IF),
                    self.read_io_u16(IME),
                );
            }
            return None;
        }
        self.hblank_pending_count.store(0, Ordering::Relaxed);
        let seq = self
            .hblank_sync
            .requested_seq
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        self.hblank_latch_vcount.store(vcount, Ordering::Relaxed);
        self.hblank_latch_seq.store(seq, Ordering::Release);
        self.prepare_fast_hblank(vcount);
        self.request_interrupt(IRQ_HBLANK);
        self.interrupt_line.pulse();
        Some(seq)
    }

    pub fn tick_scanline(&self) {
        self.advance_timers(1_232);
    }

    pub fn render_scanline(&self, y: usize) {
        if y >= HEIGHT {
            return;
        }

        let started = Instant::now();
        let mut ppu = self.ppu_from_registers();
        {
            let bg_offset = self
                .bg_offset_scanline
                .lock()
                .expect("bg offset scanline lock poisoned");
            for bg in 0..4 {
                if self.bghofs_scanline_active[bg].load(Ordering::Relaxed) {
                    ppu.write_bghofs_scanline(bg, y, bg_offset.hofs[bg][y]);
                }
                if self.bgvofs_scanline_active[bg].load(Ordering::Relaxed) {
                    ppu.write_bgvofs_scanline(bg, y, bg_offset.vofs[bg][y]);
                }
            }
        }

        let mut line = [0; WIDTH];
        ppu.render_mode0_scanline(y, self.palette.as_slice(), self.vram.as_slice(), &mut line);
        let start = y * WIDTH;
        let mut frame_buffers = self
            .frame_buffers
            .lock()
            .expect("frame buffers lock poisoned");
        frame_buffers.work[start..start + WIDTH].copy_from_slice(&line);
        self.perf
            .render_scanline_us
            .fetch_add(duration_micros(started.elapsed()), Ordering::Relaxed);
    }

    pub fn publish_completed_frame(&self) {
        let mut frame_buffers = self
            .frame_buffers
            .lock()
            .expect("frame buffers lock poisoned");
        let old_completed = frame_buffers
            .completed
            .take()
            .expect("completed frame missing");
        let next_work = match Arc::try_unwrap(old_completed) {
            Ok(frame) => frame,
            Err(_) => frame_buffers
                .spare
                .take()
                .unwrap_or_else(|| vec![0; WIDTH * HEIGHT]),
        };
        let completed = std::mem::replace(&mut frame_buffers.work, next_work);
        frame_buffers.completed = Some(Arc::new(completed));
        frame_buffers.seq = frame_buffers.seq.wrapping_add(1);
        self.completed_frame_seq
            .store(frame_buffers.seq, Ordering::Release);
        self.perf.frames.fetch_add(1, Ordering::Relaxed);
    }

    pub fn with_completed_frame<R>(&self, f: impl FnOnce(u64, &[u16]) -> R) -> R {
        let frame_buffers = self
            .frame_buffers
            .lock()
            .expect("frame buffers lock poisoned");
        let frame = frame_buffers
            .completed
            .as_ref()
            .expect("completed frame missing");
        f(frame_buffers.seq, frame.as_slice())
    }

    pub fn latest_frame_snapshot(&self) -> FrameSnapshot {
        let frame_buffers = self
            .frame_buffers
            .lock()
            .expect("frame buffers lock poisoned");
        let frame = frame_buffers
            .completed
            .as_ref()
            .expect("completed frame missing");
        FrameSnapshot {
            seq: frame_buffers.seq,
            pixels: Arc::clone(frame),
        }
    }

    pub fn completed_frame_seq(&self) -> u64 {
        self.completed_frame_seq.load(Ordering::Acquire)
    }

    pub fn hblank_irq_pending(&self) -> bool {
        self.read_io_u16(IF) & IRQ_HBLANK != 0
    }

    pub fn wait_for_hblank_complete(&self, seq: u64, timeout: Duration) -> bool {
        let started = Instant::now();
        let deadline = Instant::now() + timeout;
        let spin_deadline = started + HBLANK_COMPLETION_SPIN_BUDGET;
        loop {
            let completed_seq = self.hblank_sync.completed_seq.load(Ordering::Acquire);
            if completed_seq >= seq {
                let wait_us = duration_micros(started.elapsed());
                self.record_hblank_wait(wait_us, false);
                trace_hblank_wait(seq, wait_us, false);
                return true;
            }
            let shared_completed_seq = self.read_fast_hblank_u64(FAST_HBLANK_COMPLETED_SEQ_OFFSET);
            if shared_completed_seq >= seq {
                self.finish_fast_hblank_from_shared(seq);
                let wait_us = duration_micros(started.elapsed());
                self.record_hblank_wait(wait_us, false);
                trace_hblank_wait(seq, wait_us, false);
                return true;
            }
            if Instant::now() >= deadline {
                let wait_us = duration_micros(started.elapsed());
                self.record_hblank_wait(wait_us, true);
                trace_hblank_wait(seq, wait_us, true);
                trace_hblank_wait_timeout(
                    seq,
                    completed_seq.max(shared_completed_seq),
                    self.read_io_u16(VCOUNT),
                    self.read_io_u16(IE),
                    self.read_io_u16(IF),
                    self.read_io_u16(IME),
                );
                return false;
            }
            if Instant::now() < spin_deadline {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
    }

    pub fn record_kvm_mmio_exit(&self, addr: u32, is_write: bool, is_fast_exit: bool) {
        self.perf.kvm_mmio_exits.fetch_add(1, Ordering::Relaxed);
        if is_fast_exit {
            self.perf.kvm_mmio_fast_exit.fetch_add(1, Ordering::Relaxed);
            return;
        }

        if !(IO_START..IO_START + 0x400).contains(&addr) {
            return;
        }

        if is_write {
            self.perf.kvm_mmio_io_writes.fetch_add(1, Ordering::Relaxed);
        } else {
            self.perf.kvm_mmio_io_reads.fetch_add(1, Ordering::Relaxed);
        }

        match addr {
            IF => {
                self.perf.kvm_mmio_io_if.fetch_add(1, Ordering::Relaxed);
            }
            IME => {
                self.perf.kvm_mmio_io_ime.fetch_add(1, Ordering::Relaxed);
            }
            BG0HOFS | BG1HOFS | BG2HOFS | BG3HOFS => {
                self.perf
                    .kvm_mmio_io_bg_hofs
                    .fetch_add(1, Ordering::Relaxed);
            }
            BG0VOFS | BG1VOFS | BG2VOFS | BG3VOFS => {
                self.perf
                    .kvm_mmio_io_bg_vofs
                    .fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                self.perf.kvm_mmio_io_other.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn record_sdl_present(&self, duration: Duration) {
        self.perf
            .sdl_present_us
            .fetch_add(duration_micros(duration), Ordering::Relaxed);
    }

    pub fn fill_audio_samples(&self, out: &mut [i16]) {
        let mut audio = self.audio.lock().expect("audio lock poisoned");
        for sample in out {
            *sample = audio.pcm.pop_front().unwrap_or(0);
        }
    }

    pub fn take_video_perf_snapshot(&self) -> VideoPerfSnapshot {
        VideoPerfSnapshot {
            frames: self.perf.frames.swap(0, Ordering::Relaxed),
            render_scanline_us: self.perf.render_scanline_us.swap(0, Ordering::Relaxed),
            hblank_wait_us: self.perf.hblank_wait_us.swap(0, Ordering::Relaxed),
            hblank_wait_max_us: self.perf.hblank_wait_max_us.swap(0, Ordering::Relaxed),
            hblank_wait_timeouts: self.perf.hblank_wait_timeouts.swap(0, Ordering::Relaxed),
            fast_hblank_us: self.perf.fast_hblank_us.swap(0, Ordering::Relaxed),
            fast_hblank_count: self.perf.fast_hblank_count.swap(0, Ordering::Relaxed),
            fast_hblank_shared_count: self
                .perf
                .fast_hblank_shared_count
                .swap(0, Ordering::Relaxed),
            fast_hblank_mmio_count: self.perf.fast_hblank_mmio_count.swap(0, Ordering::Relaxed),
            kvm_mmio_exits: self.perf.kvm_mmio_exits.swap(0, Ordering::Relaxed),
            kvm_mmio_fast_exit: self.perf.kvm_mmio_fast_exit.swap(0, Ordering::Relaxed),
            kvm_mmio_io_reads: self.perf.kvm_mmio_io_reads.swap(0, Ordering::Relaxed),
            kvm_mmio_io_writes: self.perf.kvm_mmio_io_writes.swap(0, Ordering::Relaxed),
            kvm_mmio_io_if: self.perf.kvm_mmio_io_if.swap(0, Ordering::Relaxed),
            kvm_mmio_io_ime: self.perf.kvm_mmio_io_ime.swap(0, Ordering::Relaxed),
            kvm_mmio_io_bg_hofs: self.perf.kvm_mmio_io_bg_hofs.swap(0, Ordering::Relaxed),
            kvm_mmio_io_bg_vofs: self.perf.kvm_mmio_io_bg_vofs.swap(0, Ordering::Relaxed),
            kvm_mmio_io_other: self.perf.kvm_mmio_io_other.swap(0, Ordering::Relaxed),
            sdl_present_us: self.perf.sdl_present_us.swap(0, Ordering::Relaxed),
        }
    }

    fn record_hblank_wait(&self, wait_us: u64, timeout: bool) {
        self.perf
            .hblank_wait_us
            .fetch_add(wait_us, Ordering::Relaxed);
        self.perf
            .hblank_wait_max_us
            .fetch_max(wait_us, Ordering::Relaxed);
        if timeout {
            self.perf
                .hblank_wait_timeouts
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn set_keyinput(&self, value: u16) {
        let old = self.read_io_u16(KEYINPUT);
        self.write_io_u16(KEYINPUT, value);
        if old != value {
            trace_input_keyinput(value);
        }
    }

    fn prepare_fast_hblank(&self, vcount: u16) {
        let io_len = self.io.as_slice().len();
        let shadow_offset = fast_offset(SHADOW_IO_ADDR);
        self.fast.as_mut_slice()[shadow_offset..shadow_offset + io_len]
            .copy_from_slice(self.io.as_slice());
        let seq = self.hblank_latch_seq.load(Ordering::Acquire);
        self.write_fast_hblank_u64(FAST_HBLANK_REQUESTED_SEQ_OFFSET, seq);
        self.write_fast_hblank_u64(FAST_HBLANK_COMPLETED_SEQ_OFFSET, 0);
        self.write_fast_hblank_u16(FAST_HBLANK_VCOUNT_OFFSET, vcount);
        self.write_fast_hblank_u32(FAST_HBLANK_DIRTY_MASK_OFFSET, 0);
        self.write_shadow_io_u16(VCOUNT, vcount);
        self.write_shadow_io_u16(IF, self.read_io_u16(IF) | IRQ_HBLANK);
        self.write_shadow_io_u16(DISPSTAT, self.read_io_u16(DISPSTAT) | DISPSTAT_HBLANK);
        self.write_fast_hblank_u32(FAST_HBLANK_KIND_OFFSET, FAST_CTRL_HBLANK);
    }

    pub fn finish_fast_hblank(&self) {
        let started = Instant::now();
        self.write_fast_hblank_u32(FAST_HBLANK_KIND_OFFSET, FAST_CTRL_NONE);
        let ack = self.read_shadow_io_u16(IF) | IRQ_HBLANK;
        let bg1hofs = self.read_shadow_io_u16(BG1HOFS);
        let vcount = self.hblank_latch_vcount.load(Ordering::Relaxed);
        self.write_io_u16(IF, self.read_io_u16(IF) & !ack);
        self.write_io_u16(IME, self.read_shadow_io_u16(IME));

        let target_line = self.hblank_target_line();
        let mut bg_offset = self
            .bg_offset_scanline
            .lock()
            .expect("bg offset scanline lock poisoned");
        for (index, register) in [BG0HOFS, BG1HOFS, BG2HOFS, BG3HOFS].into_iter().enumerate() {
            let value = self.read_shadow_io_u16(register);
            self.write_io_u16(register, value);
            if let Some(line) = target_line {
                bg_offset.hofs[index][line] = value;
                self.bghofs_scanline_active[index].store(true, Ordering::Relaxed);
            }
        }
        for (index, register) in [BG0VOFS, BG1VOFS, BG2VOFS, BG3VOFS].into_iter().enumerate() {
            let value = self.read_shadow_io_u16(register);
            self.write_io_u16(register, value);
            if let Some(line) = target_line {
                bg_offset.vofs[index][line] = value;
                self.bgvofs_scanline_active[index].store(true, Ordering::Relaxed);
            }
        }
        drop(bg_offset);
        let seq = self.hblank_latch_seq.load(Ordering::Acquire);
        self.hblank_sync.completed_seq.store(seq, Ordering::Release);
        trace_fast_hblank(vcount, bg1hofs, ack);
        self.update_interrupt_line();
        self.perf
            .fast_hblank_us
            .fetch_add(duration_micros(started.elapsed()), Ordering::Relaxed);
        self.perf.fast_hblank_count.fetch_add(1, Ordering::Relaxed);
        self.perf
            .fast_hblank_mmio_count
            .fetch_add(1, Ordering::Relaxed);
    }

    fn finish_fast_hblank_from_shared(&self, seq: u64) {
        let started = Instant::now();
        self.write_fast_hblank_u32(FAST_HBLANK_KIND_OFFSET, FAST_CTRL_NONE);
        let dirty_mask = self.read_fast_hblank_u32(FAST_HBLANK_DIRTY_MASK_OFFSET);
        let ack = if dirty_mask & FAST_HBLANK_DIRTY_IF_ACK != 0 {
            self.read_fast_hblank_u16(FAST_HBLANK_IF_ACK_OFFSET)
        } else {
            IRQ_HBLANK
        };
        let bg1hofs = self.read_fast_hblank_u16(FAST_HBLANK_BG_HOFS_OFFSET + 2);
        let vcount = self.read_fast_hblank_u16(FAST_HBLANK_VCOUNT_OFFSET);
        self.write_io_u16(IF, self.read_io_u16(IF) & !ack);
        if dirty_mask & FAST_HBLANK_DIRTY_IME != 0 {
            self.write_io_u16(IME, self.read_fast_hblank_u16(FAST_HBLANK_IME_OFFSET));
        }

        let target_line = self.hblank_target_line();
        let mut bg_offset = self
            .bg_offset_scanline
            .lock()
            .expect("bg offset scanline lock poisoned");
        for index in 0..4 {
            if dirty_mask & (FAST_HBLANK_DIRTY_BG_HOFS0 << index) != 0 {
                let value = self.read_fast_hblank_u16(FAST_HBLANK_BG_HOFS_OFFSET + index * 2);
                self.write_io_u16([BG0HOFS, BG1HOFS, BG2HOFS, BG3HOFS][index], value);
                if let Some(line) = target_line {
                    bg_offset.hofs[index][line] = value;
                    self.bghofs_scanline_active[index].store(true, Ordering::Relaxed);
                }
            }
        }
        for index in 0..4 {
            if dirty_mask & (FAST_HBLANK_DIRTY_BG_VOFS0 << index) != 0 {
                let value = self.read_fast_hblank_u16(FAST_HBLANK_BG_VOFS_OFFSET + index * 2);
                self.write_io_u16([BG0VOFS, BG1VOFS, BG2VOFS, BG3VOFS][index], value);
                if let Some(line) = target_line {
                    bg_offset.vofs[index][line] = value;
                    self.bgvofs_scanline_active[index].store(true, Ordering::Relaxed);
                }
            }
        }
        drop(bg_offset);
        self.hblank_sync.completed_seq.store(seq, Ordering::Release);
        trace_fast_hblank(vcount, bg1hofs, ack);
        self.update_interrupt_line();
        self.perf
            .fast_hblank_us
            .fetch_add(duration_micros(started.elapsed()), Ordering::Relaxed);
        self.perf.fast_hblank_count.fetch_add(1, Ordering::Relaxed);
        self.perf
            .fast_hblank_shared_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn render_frame(&self) -> FrameBuffer {
        let mut ppu = self.ppu_from_registers();
        let bg_offset = self
            .bg_offset_scanline
            .lock()
            .expect("bg offset scanline lock poisoned");
        for bg in 0..4 {
            if self.bghofs_scanline_active[bg].load(Ordering::Relaxed) {
                for y in 0..HEIGHT {
                    ppu.write_bghofs_scanline(bg, y, bg_offset.completed_hofs[bg][y]);
                }
            }
            if self.bgvofs_scanline_active[bg].load(Ordering::Relaxed) {
                for y in 0..HEIGHT {
                    ppu.write_bgvofs_scanline(bg, y, bg_offset.completed_vofs[bg][y]);
                }
            }
        }
        ppu.write_bgpa(2, self.read_io_u16(BG2PA));
        ppu.write_bgpb(2, self.read_io_u16(BG2PB));
        ppu.write_bgpc(2, self.read_io_u16(BG2PC));
        ppu.write_bgpd(2, self.read_io_u16(BG2PD));
        ppu.write_bgx(2, self.read_io_u32(BG2X));
        ppu.write_bgy(2, self.read_io_u32(BG2Y));
        ppu.write_winh(0, self.read_io_u16(WIN0H));
        ppu.write_winh(1, self.read_io_u16(WIN1H));
        ppu.write_winv(0, self.read_io_u16(WIN0V));
        ppu.write_winv(1, self.read_io_u16(WIN1V));
        ppu.write_winin(self.read_io_u16(WININ));
        ppu.write_winout(self.read_io_u16(WINOUT));
        ppu.write_mosaic(self.read_io_u16(MOSAIC));
        ppu.write_bldcnt(self.read_io_u16(BLDCNT));
        ppu.write_bldalpha(self.read_io_u16(BLDALPHA));
        ppu.write_bldy(self.read_io_u16(BLDY));
        ppu.render_frame(
            self.palette.as_slice(),
            self.vram.as_slice(),
            self.oam.as_slice(),
        )
    }

    fn ppu_from_registers(&self) -> Ppu {
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
        ppu.write_bgpa(2, self.read_io_u16(BG2PA));
        ppu.write_bgpb(2, self.read_io_u16(BG2PB));
        ppu.write_bgpc(2, self.read_io_u16(BG2PC));
        ppu.write_bgpd(2, self.read_io_u16(BG2PD));
        ppu.write_bgx(2, self.read_io_u32(BG2X));
        ppu.write_bgy(2, self.read_io_u32(BG2Y));
        ppu.write_winh(0, self.read_io_u16(WIN0H));
        ppu.write_winh(1, self.read_io_u16(WIN1H));
        ppu.write_winv(0, self.read_io_u16(WIN0V));
        ppu.write_winv(1, self.read_io_u16(WIN1V));
        ppu.write_winin(self.read_io_u16(WININ));
        ppu.write_winout(self.read_io_u16(WINOUT));
        ppu.write_mosaic(self.read_io_u16(MOSAIC));
        ppu.write_bldcnt(self.read_io_u16(BLDCNT));
        ppu.write_bldalpha(self.read_io_u16(BLDALPHA));
        ppu.write_bldy(self.read_io_u16(BLDY));
        ppu
    }

    pub fn supports_scanline_renderer(&self) -> bool {
        let dispcnt = self.read_io_u16(DISPCNT);
        if dispcnt & DISPCNT_MODE_MASK != MODE_0 {
            return false;
        }
        if dispcnt & (WIN0_ENABLE | WIN1_ENABLE | OBJ_WIN_ENABLE) != 0 {
            return false;
        }
        if dispcnt & OBJ_ENABLE != 0 && self.has_renderable_obj() {
            return false;
        }
        if self.read_io_u16(BLDCNT) != 0 {
            return false;
        }

        for (bg, enable) in [BG0_ENABLE, BG1_ENABLE, BG2_ENABLE, BG3_ENABLE]
            .into_iter()
            .enumerate()
        {
            if dispcnt & enable != 0
                && self.read_io_u16([BG0CNT, BG1CNT, BG2CNT, BG3CNT][bg]) & (1 << 6) != 0
            {
                return false;
            }
        }
        true
    }

    pub fn needs_scanline_renderer(&self) -> bool {
        if self.read_io_u16(DISPSTAT) & DISPSTAT_HBLANK_IRQ_ENABLE != 0
            && self.read_io_u16(IE) & IRQ_HBLANK != 0
            && self.read_io_u16(IME) & 1 != 0
        {
            return true;
        }
        self.bghofs_scanline_active
            .iter()
            .any(|active| active.load(Ordering::Relaxed))
            || self
                .bgvofs_scanline_active
                .iter()
                .any(|active| active.load(Ordering::Relaxed))
    }

    fn has_renderable_obj(&self) -> bool {
        self.oam.as_slice().chunks_exact(8).take(128).any(|obj| {
            let attr0 = u16::from_le_bytes([obj[0], obj[1]]);
            let affine = attr0 & (1 << 8) != 0;
            if !affine && attr0 & (1 << 9) != 0 {
                return false;
            }
            if ((attr0 >> 10) & 0x3) != 0 {
                return false;
            }
            attr0 & (1 << 13) == 0
        })
    }

    pub fn debug_video_state(&self) -> KvmVideoDebugState {
        let bg_offset = self
            .bg_offset_scanline
            .lock()
            .expect("bg offset scanline lock poisoned");
        let bg1_min = bg_offset.completed_hofs[1]
            .iter()
            .copied()
            .min()
            .unwrap_or(0);
        let bg1_max = bg_offset.completed_hofs[1]
            .iter()
            .copied()
            .max()
            .unwrap_or(0);
        let bg1_sample = bg_offset.completed_hofs[1][80];
        let bg1_checksum = bg_offset.completed_hofs[1]
            .iter()
            .fold(0u32, |sum, value| sum.wrapping_add(u32::from(*value)));
        drop(bg_offset);
        KvmVideoDebugState {
            dispcnt: self.read_io_u16(DISPCNT),
            bg0cnt: self.read_io_u16(BG0CNT),
            bg1cnt: self.read_io_u16(BG1CNT),
            bg2cnt: self.read_io_u16(BG2CNT),
            bg0hofs: self.read_io_u16(BG0HOFS),
            bg1hofs: self.read_io_u16(BG1HOFS),
            keyinput: self.read_io_u16(KEYINPUT),
            irq_waitflags: self.read_iwram_u16(0x7ff8),
            mosaic: self.read_io_u16(MOSAIC),
            dma3cnt: self.read_io_u16(DMA0CNT + 3 * 12 + 2),
            vram0: u16::from_le_bytes([self.vram.as_slice()[0], self.vram.as_slice()[1]]),
            bg0_map_nonzero: self.count_bg_map_nonzero(29),
            bg0_text_1: self.read_vram_u16(29 * 0x800 + (1 + 1 * 32) * 2),
            bg0_text_2: self.read_vram_u16(29 * 0x800 + (1 + 2 * 32) * 2),
            cycle_digit_10: self.read_vram_u16(29 * 0x800 + (11 + 1 * 32) * 2),
            cycle_digit_1: self.read_vram_u16(29 * 0x800 + (12 + 1 * 32) * 2),
            cx_digit_10: self.read_vram_u16(29 * 0x800 + (11 + 2 * 32) * 2),
            cx_digit_1: self.read_vram_u16(29 * 0x800 + (12 + 2 * 32) * 2),
            bg1_raster_min: bg1_min,
            bg1_raster_max: bg1_max,
            bg1_raster_sample: bg1_sample,
            bg1_raster_checksum: bg1_checksum,
        }
    }

    fn read_iwram_u16(&self, offset: usize) -> u16 {
        let iwram = self.iwram.as_slice();
        u16::from_le_bytes([iwram[offset], iwram[offset + 1]])
    }

    fn read_vram_u16(&self, offset: usize) -> u16 {
        let vram = self.vram.as_slice();
        u16::from_le_bytes([vram[offset], vram[offset + 1]])
    }

    fn count_bg_map_nonzero(&self, screen_block: usize) -> usize {
        let base = screen_block * 0x800;
        self.vram.as_slice()[base..base + 0x800]
            .chunks_exact(2)
            .filter(|entry| u16::from_le_bytes([entry[0], entry[1]]) != 0)
            .count()
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

    fn read_shadow_io_u16(&self, addr: u32) -> u16 {
        let offset = fast_offset(SHADOW_IO_ADDR) + (addr - IO_START) as usize;
        let fast = self.fast.as_slice();
        u16::from_le_bytes([fast[offset], fast[offset + 1]])
    }

    fn write_shadow_io_u16(&self, addr: u32, value: u16) {
        let offset = fast_offset(SHADOW_IO_ADDR) + (addr - IO_START) as usize;
        let bytes = value.to_le_bytes();
        let fast = self.fast.as_mut_slice();
        fast[offset] = bytes[0];
        fast[offset + 1] = bytes[1];
    }

    fn read_fast_hblank_u16(&self, offset: usize) -> u16 {
        let base = fast_offset(FAST_CTRL_ADDR) + offset;
        let fast = self.fast.as_slice();
        unsafe { ptr::read_volatile(fast.as_ptr().add(base).cast::<u16>()) }
    }

    fn read_fast_hblank_u32(&self, offset: usize) -> u32 {
        let base = fast_offset(FAST_CTRL_ADDR) + offset;
        let fast = self.fast.as_slice();
        unsafe { ptr::read_volatile(fast.as_ptr().add(base).cast::<u32>()) }
    }

    fn read_fast_hblank_u64(&self, offset: usize) -> u64 {
        let low = u64::from(self.read_fast_hblank_u32(offset));
        let high = u64::from(self.read_fast_hblank_u32(offset + 4));
        low | (high << 32)
    }

    fn write_fast_hblank_u16(&self, offset: usize, value: u16) {
        let base = fast_offset(FAST_CTRL_ADDR) + offset;
        let fast = self.fast.as_mut_slice();
        unsafe { ptr::write_volatile(fast.as_mut_ptr().add(base).cast::<u16>(), value) };
    }

    fn write_fast_hblank_u32(&self, offset: usize, value: u32) {
        let base = fast_offset(FAST_CTRL_ADDR) + offset;
        let fast = self.fast.as_mut_slice();
        unsafe { ptr::write_volatile(fast.as_mut_ptr().add(base).cast::<u32>(), value) };
    }

    fn write_fast_hblank_u64(&self, offset: usize, value: u64) {
        self.write_fast_hblank_u32(offset, value as u32);
        self.write_fast_hblank_u32(offset + 4, (value >> 32) as u32);
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
        self.handle_audio_io_write(addr, len as u32, data);
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
        let overflows = self
            .timers
            .lock()
            .expect("timer lock poisoned")
            .advance(cycles, &self.io);
        for (timer, count) in overflows.into_iter().enumerate() {
            for _ in 0..count {
                self.on_timer_overflow(timer);
            }
        }
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

    fn hblank_target_line(&self) -> Option<usize> {
        let vcount = self.hblank_latch_vcount.load(Ordering::Relaxed);
        match vcount {
            0..=158 => Some(usize::from(vcount + 1)),
            227 => Some(0),
            _ => None,
        }
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

    fn run_fifo_dma_for_timer(&self, timer: usize) {
        for channel in 1..=2 {
            let base = DMA0SAD + channel as u32 * 12;
            let control = self.read_io_u16(base + 10);
            if control & DMA_ENABLE == 0
                || control & DMA_TIMING_MASK != DMA_TIMING_SPECIAL
                || control & DMA_32BIT == 0
            {
                continue;
            }

            let destination = self.read_io_u32(base + 4);
            let fifo = match destination {
                FIFO_A => 0,
                FIFO_B => 1,
                _ => continue,
            };
            if self.sound_fifo_timer(fifo) != timer {
                continue;
            }
            if self.sound_fifo_len(fifo) > AUDIO_FIFO_DMA_THRESHOLD {
                continue;
            }

            let mut source = self.read_io_u32(base);
            for _ in 0..4 {
                let low = self.dma_read_halfword(source);
                let high = self.dma_read_halfword(source.wrapping_add(2));
                self.push_fifo_word(fifo, low, high);
                source = source.wrapping_add(4);
            }
            self.write_io_u16(base, source as u16);
            self.write_io_u16(base + 2, (source >> 16) as u16);
        }
    }

    fn on_timer_overflow(&self, timer: usize) {
        self.run_fifo_dma_for_timer(timer);
        let soundcnt_h = self.read_io_u16(SOUNDCNT_H);
        let mut audio = self.audio.lock().expect("audio lock poisoned");
        let mut left = 0i32;
        let mut right = 0i32;

        if self.sound_fifo_timer(0) == timer {
            let sample = i32::from(audio.fifo_a.pop_front().unwrap_or(0)) << 8;
            if soundcnt_h & SOUND_A_LEFT != 0 {
                left += sample;
            }
            if soundcnt_h & SOUND_A_RIGHT != 0 {
                right += sample;
            }
        }
        if self.sound_fifo_timer(1) == timer {
            let sample = i32::from(audio.fifo_b.pop_front().unwrap_or(0)) << 8;
            if soundcnt_h & SOUND_B_LEFT != 0 {
                left += sample;
            }
            if soundcnt_h & SOUND_B_RIGHT != 0 {
                right += sample;
            }
        }

        push_pcm_sample(&mut audio.pcm, left);
        push_pcm_sample(&mut audio.pcm, right);
    }

    fn sound_fifo_timer(&self, fifo: usize) -> usize {
        let soundcnt_h = self.read_io_u16(SOUNDCNT_H);
        if fifo == 0 {
            usize::from((soundcnt_h & SOUND_A_TIMER) != 0)
        } else {
            usize::from((soundcnt_h & SOUND_B_TIMER) != 0)
        }
    }

    fn sound_fifo_len(&self, fifo: usize) -> usize {
        let audio = self.audio.lock().expect("audio lock poisoned");
        if fifo == 0 {
            audio.fifo_a.len()
        } else {
            audio.fifo_b.len()
        }
    }

    fn handle_audio_io_write(&self, addr: u32, len: u32, data: &[u8; 8]) {
        if let Some(write) = io_register_write(SOUNDCNT_H, addr, len, data) {
            let mut audio = self.audio.lock().expect("audio lock poisoned");
            if write.value & SOUND_A_RESET != 0 {
                audio.fifo_a.clear();
            }
            if write.value & SOUND_B_RESET != 0 {
                audio.fifo_b.clear();
            }
        }

        for byte_index in 0..len as usize {
            let byte_addr = addr + byte_index as u32;
            if (FIFO_A..FIFO_A + 4).contains(&byte_addr) {
                self.push_fifo_byte(0, data[byte_index] as i8);
            } else if (FIFO_B..FIFO_B + 4).contains(&byte_addr) {
                self.push_fifo_byte(1, data[byte_index] as i8);
            }
        }
    }

    fn push_fifo_word(&self, fifo: usize, low: u16, high: u16) {
        for byte in low.to_le_bytes().into_iter().chain(high.to_le_bytes()) {
            self.push_fifo_byte(fifo, byte as i8);
        }
    }

    fn push_fifo_byte(&self, fifo: usize, sample: i8) {
        let mut audio = self.audio.lock().expect("audio lock poisoned");
        let queue = if fifo == 0 {
            &mut audio.fifo_a
        } else {
            &mut audio.fifo_b
        };
        if queue.len() >= AUDIO_FIFO_CAPACITY {
            queue.pop_front();
        }
        queue.push_back(sample);
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
        if (FIFO_A..FIFO_A + 4).contains(&addr) {
            self.push_fifo_byte(0, value as u8 as i8);
            self.push_fifo_byte(0, (value >> 8) as u8 as i8);
            return;
        }
        if (FIFO_B..FIFO_B + 4).contains(&addr) {
            self.push_fifo_byte(1, value as u8 as i8);
            self.push_fifo_byte(1, (value >> 8) as u8 as i8);
            return;
        }
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
    pub bg0cnt: u16,
    pub bg1cnt: u16,
    pub bg2cnt: u16,
    pub bg0hofs: u16,
    pub bg1hofs: u16,
    pub keyinput: u16,
    pub irq_waitflags: u16,
    pub mosaic: u16,
    pub dma3cnt: u16,
    pub vram0: u16,
    pub bg0_map_nonzero: usize,
    pub bg0_text_1: u16,
    pub bg0_text_2: u16,
    pub cycle_digit_10: u16,
    pub cycle_digit_1: u16,
    pub cx_digit_10: u16,
    pub cx_digit_1: u16,
    pub bg1_raster_min: u16,
    pub bg1_raster_max: u16,
    pub bg1_raster_sample: u16,
    pub bg1_raster_checksum: u32,
}

const DISPSTAT_VBLANK: u16 = 1 << 0;
const DISPSTAT_HBLANK: u16 = 1 << 1;
const DISPSTAT_STATUS_MASK: u16 = 0x0007;
const DISPSTAT_WRITABLE_MASK: u16 = 0xff38;
const DISPSTAT_VBLANK_IRQ_ENABLE: u16 = 1 << 3;
const DISPSTAT_HBLANK_IRQ_ENABLE: u16 = 1 << 4;
const IRQ_VBLANK: u16 = 1 << 0;
const IRQ_HBLANK: u16 = 1 << 1;
const FAST_CTRL_NONE: u32 = 0;
const FAST_CTRL_HBLANK: u32 = 1;
const FAST_HBLANK_KIND_OFFSET: usize = 0;
const FAST_HBLANK_REQUESTED_SEQ_OFFSET: usize = 4;
const FAST_HBLANK_COMPLETED_SEQ_OFFSET: usize = 12;
const FAST_HBLANK_VCOUNT_OFFSET: usize = 20;
const FAST_HBLANK_DIRTY_MASK_OFFSET: usize = 24;
const FAST_HBLANK_BG_HOFS_OFFSET: usize = 28;
const FAST_HBLANK_BG_VOFS_OFFSET: usize = 36;
const FAST_HBLANK_IME_OFFSET: usize = 44;
const FAST_HBLANK_IF_ACK_OFFSET: usize = 46;
const FAST_HBLANK_DIRTY_BG_HOFS0: u32 = 1 << 0;
const FAST_HBLANK_DIRTY_BG_VOFS0: u32 = 1 << 4;
const FAST_HBLANK_DIRTY_IME: u32 = 1 << 8;
const FAST_HBLANK_DIRTY_IF_ACK: u32 = 1 << 9;
const HBLANK_COMPLETION_SPIN_BUDGET: Duration = Duration::from_micros(80);
const DMA_ENABLE: u16 = 1 << 15;
const DMA_REPEAT: u16 = 1 << 9;
const DMA_32BIT: u16 = 1 << 10;
const DMA_TIMING_MASK: u16 = 0x3000;
const DMA_TIMING_SPECIAL: u16 = 0x3000;
const SOUND_A_RIGHT: u16 = 1 << 8;
const SOUND_A_LEFT: u16 = 1 << 9;
const SOUND_A_TIMER: u16 = 1 << 10;
const SOUND_A_RESET: u16 = 1 << 11;
const SOUND_B_RIGHT: u16 = 1 << 12;
const SOUND_B_LEFT: u16 = 1 << 13;
const SOUND_B_TIMER: u16 = 1 << 14;
const SOUND_B_RESET: u16 = 1 << 15;
const AUDIO_FIFO_CAPACITY: usize = 32;
const AUDIO_FIFO_DMA_THRESHOLD: usize = 16;
const AUDIO_PCM_CAPACITY: usize = 16_384;

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

        self.set_level(asserted);
    }

    fn pulse(&self) {
        self.asserted.store(true, Ordering::Relaxed);
        self.set_level(false);
        self.set_level(true);
    }

    fn set_level(&self, asserted: bool) {
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

fn fast_offset(addr: u32) -> usize {
    (addr - FAST_MEM_START) as usize
}

fn duration_micros(duration: Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

fn push_pcm_sample(pcm: &mut VecDeque<i16>, sample: i32) {
    if pcm.len() >= AUDIO_PCM_CAPACITY {
        pcm.pop_front();
    }
    pcm.push_back(sample.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16);
}

#[cfg(test)]
mod tests {
    use super::{
        FAST_HBLANK_BG_HOFS_OFFSET, FAST_HBLANK_BG_VOFS_OFFSET, FAST_HBLANK_COMPLETED_SEQ_OFFSET,
        FAST_HBLANK_DIRTY_BG_HOFS0, FAST_HBLANK_DIRTY_BG_VOFS0, FAST_HBLANK_DIRTY_IF_ACK,
        FAST_HBLANK_DIRTY_IME, FAST_HBLANK_DIRTY_MASK_OFFSET, FAST_HBLANK_IF_ACK_OFFSET,
        FAST_HBLANK_IME_OFFSET, GAME_PAK_ROM_START, IoRegisterWrite, KvmSharedMemory, MOSAIC,
        PALETTE_SIZE, VRAM_SIZE, io_register_write,
    };
    use crate::{
        gba::memory_map::{
            BG1HOFS, BG1VOFS, DMA2CNT, DMA2DAD, DMA2SAD, DMA3CNT, DMA3DAD, DMA3SAD, EWRAM_SIZE,
            FIFO_B, IF, IME, IO_START, IWRAM_SIZE, OAM_SIZE, SOUNDCNT_H,
        },
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

    #[test]
    fn shared_hblank_completion_updates_mirror_and_raster_state() {
        let shared = test_shared_memory(&[]);
        shared.write_io_u16(IF, 0x0002);
        shared
            .hblank_latch_vcount
            .store(7, std::sync::atomic::Ordering::Relaxed);
        shared
            .hblank_latch_seq
            .store(3, std::sync::atomic::Ordering::Release);
        shared.write_fast_hblank_u32(
            FAST_HBLANK_DIRTY_MASK_OFFSET,
            (FAST_HBLANK_DIRTY_BG_HOFS0 << 1)
                | (FAST_HBLANK_DIRTY_BG_VOFS0 << 1)
                | FAST_HBLANK_DIRTY_IME
                | FAST_HBLANK_DIRTY_IF_ACK,
        );
        shared.write_fast_hblank_u16(FAST_HBLANK_BG_HOFS_OFFSET + 2, 0x0042);
        shared.write_fast_hblank_u16(FAST_HBLANK_BG_VOFS_OFFSET + 2, 0x0017);
        shared.write_fast_hblank_u16(FAST_HBLANK_IME_OFFSET, 1);
        shared.write_fast_hblank_u16(FAST_HBLANK_IF_ACK_OFFSET, 0x0002);
        shared.write_fast_hblank_u64(FAST_HBLANK_COMPLETED_SEQ_OFFSET, 3);

        assert!(shared.wait_for_hblank_complete(3, std::time::Duration::from_millis(1)));

        assert_eq!(shared.read_io_u16(IF) & 0x0002, 0);
        assert_eq!(shared.read_io_u16(IME), 1);
        assert_eq!(shared.read_io_u16(BG1HOFS), 0x0042);
        assert_eq!(shared.read_io_u16(BG1VOFS), 0x0017);
        let bg_offset = shared
            .bg_offset_scanline
            .lock()
            .expect("bg offset scanline lock poisoned");
        assert_eq!(bg_offset.hofs[1][8], 0x0042);
        assert_eq!(bg_offset.vofs[1][8], 0x0017);
    }

    #[test]
    fn timer_overflow_runs_fifo_dma_and_publishes_pcm() {
        let rom = [0x40, 0xc0, 0x20, 0xe0, 0x10, 0xf0, 0x08, 0xf8];
        let shared = test_shared_memory(&rom);
        shared.write_io_u16(SOUNDCNT_H, 0x7000);
        shared.write_io_u16(DMA2SAD, GAME_PAK_ROM_START as u16);
        shared.write_io_u16(DMA2SAD + 2, (GAME_PAK_ROM_START >> 16) as u16);
        shared.write_io_u16(DMA2DAD, FIFO_B as u16);
        shared.write_io_u16(DMA2DAD + 2, (FIFO_B >> 16) as u16);
        shared.write_io_u16(DMA2CNT, 0);
        shared.write_io_u16(DMA2CNT + 2, 0xb600);
        shared.write_io_u16(IO_START + 0x0104, 0xffff);
        shared.write_io_u16(IO_START + 0x0106, 0x0080);
        shared.write_timer_registers_from_io(IO_START + 0x0104, 4);

        shared.advance_timers(1);

        let mut samples = [0; 2];
        shared.fill_audio_samples(&mut samples);
        assert_eq!(samples, [0x4000, 0x4000]);
        assert_eq!(
            shared.read_io_u16(DMA2SAD),
            (GAME_PAK_ROM_START + 16) as u16
        );
    }

    fn test_shared_memory(rom: &[u8]) -> KvmSharedMemory {
        KvmSharedMemory::new(
            MemoryRegion::anonymous(EWRAM_SIZE).expect("ewram"),
            MemoryRegion::anonymous(IWRAM_SIZE).expect("iwram"),
            MemoryRegion::anonymous(0x1000).expect("io"),
            MemoryRegion::anonymous(PALETTE_SIZE).expect("palette"),
            MemoryRegion::anonymous(VRAM_SIZE).expect("vram"),
            MemoryRegion::anonymous(OAM_SIZE).expect("oam"),
            MemoryRegion::anonymous(0x10000).expect("fast"),
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

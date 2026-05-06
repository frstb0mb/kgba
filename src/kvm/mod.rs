mod sys;

use std::{
    env,
    os::fd::RawFd,
    ptr,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use crate::gba::{
    cartridge::Cartridge,
    memory_map::{
        BIOS_SIZE, BIOS_START, DISPCNT, EWRAM_SIZE, EWRAM_START, GAME_PAK_ROM_START, IO_SIZE,
        IO_START, IWRAM_SIZE, IWRAM_START, OAM_START, PALETTE_START, VCOUNT, VRAM_SIZE, VRAM_START,
    },
    ppu::{FrameBuffer, Ppu},
};

const IO_SLOT_SIZE: usize = 0x1000;
const PALETTE_SLOT_SIZE: usize = 0x1000;
const OAM_SLOT_SIZE: usize = 0x1000;
const PAGE_TABLE_GUEST_ADDR: u32 = IWRAM_START + 0x4000;
const PAGE_TABLE_IWRAM_OFFSET: usize = 0x4000;
const SECTION_SIZE: u32 = 0x0010_0000;
const TTBR0_INNER_SHAREABLE_WBWA: u32 = (1 << 6) | (1 << 3) | (1 << 1);
const TTBR0_VALUE: u32 = PAGE_TABLE_GUEST_ADDR | TTBR0_INNER_SHAREABLE_WBWA;
const BIOS_RESET_HANDLER_OFFSET: usize = 0x40;
const BIOS_SWI_HANDLER_OFFSET: usize = 0x100;

const SECTION_DESCRIPTOR: u32 = 0b10;
const SECTION_BUFFERABLE: u32 = 1 << 2;
const SECTION_CACHEABLE: u32 = 1 << 3;
const SECTION_AP_FULL_ACCESS: u32 = 0b11 << 10;
const SECTION_TEX_WRITE_BACK_WRITE_ALLOCATE: u32 = 0b001 << 12;
const SECTION_SHAREABLE: u32 = 1 << 16;

const NORMAL_SHARED_WBWA_SECTION: u32 = SECTION_DESCRIPTOR
    | SECTION_BUFFERABLE
    | SECTION_CACHEABLE
    | SECTION_AP_FULL_ACCESS
    | SECTION_TEX_WRITE_BACK_WRITE_ALLOCATE
    | SECTION_SHAREABLE;
const CACHE_BOOTSTRAP: [u32; 15] = [
    0xe59f_0028, // ldr r0, [pc, #0x28] ; TTBR0
    0xee02_0f10, // mcr p15, 0, r0, c2, c0, 0
    0xe3e0_0000, // mvn r0, #0 ; DACR all manager
    0xee03_0f10, // mcr p15, 0, r0, c3, c0, 0
    0xee11_0f10, // mrc p15, 0, r0, c1, c0, 0 ; SCTLR
    0xe59f_1018, // ldr r1, [pc, #0x18] ; M | C | I
    0xe180_0001, // orr r0, r0, r1
    0xee01_0f10, // mcr p15, 0, r0, c1, c0, 0
    0xf57f_f04f, // dsb sy
    0xf57f_f06f, // isb sy
    0xe59f_0008, // ldr r0, [pc, #0x08] ; ROM entry
    0xe12f_ff10, // bx r0
    TTBR0_VALUE,
    0x0000_1005, // SCTLR.M | SCTLR.C | SCTLR.I
    GAME_PAK_ROM_START,
];

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

#[derive(Debug)]
pub struct KvmSharedMemory {
    io: MemoryRegion,
    palette: MemoryRegion,
    vram: MemoryRegion,
    oam: MemoryRegion,
    timers: Mutex<Timers>,
}

unsafe impl Send for KvmSharedMemory {}
unsafe impl Sync for KvmSharedMemory {}

impl KvmSharedMemory {
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

    fn read_io_u16(&self, addr: u32) -> u16 {
        let offset = (addr - IO_START) as usize;
        let io = self.io.as_slice();
        u16::from_le_bytes([io[offset], io[offset + 1]])
    }

    fn write_io_u16(&self, addr: u32, value: u16) {
        let offset = (addr - IO_START) as usize;
        let bytes = value.to_le_bytes();
        let io = self.io.as_mut_slice();
        io[offset] = bytes[0];
        io[offset + 1] = bytes[1];
    }

    fn mirror_io_write(&self, addr: u32, len: u32, data: &[u8; 8]) {
        let offset = (addr - IO_START) as usize;
        let len = len as usize;
        self.io.as_mut_slice()[offset..offset + len].copy_from_slice(&data[..len]);
    }

    fn copy_io_read(&self, addr: u32, len: u32, data: &mut [u8; 8]) {
        let offset = (addr - IO_START) as usize;
        let len = len as usize;
        data.fill(0);
        data[..len].copy_from_slice(&self.io.as_slice()[offset..offset + len]);
    }

    fn write_timer_registers_from_io(&self, addr: u32, len: u32) {
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

pub struct KvmGba {
    kvm_fd: Fd,
    vm_fd: Fd,
    vcpu_fd: Fd,
    run: RunMapping,
    _slots: Vec<MemorySlot>,
    shared: Arc<KvmSharedMemory>,
}

unsafe impl Send for KvmGba {}

impl KvmGba {
    pub fn new(cartridge: &Cartridge) -> Result<Self, String> {
        let kvm_fd = Fd::open("/dev/kvm")?;
        let api = unsafe { sys::ioctl_noarg(kvm_fd.raw(), sys::KVM_GET_API_VERSION) };
        if api != sys::KVM_API_VERSION {
            return Err(format!(
                "unsupported KVM API version: got {api}, expected {}",
                sys::KVM_API_VERSION
            ));
        }

        let vm_raw = unsafe { sys::ioctl_arg(kvm_fd.raw(), sys::KVM_CREATE_VM, 0) };
        if vm_raw < 0 {
            return Err(last_os_error("KVM_CREATE_VM"));
        }
        let vm_fd = Fd(vm_raw);

        let mut slot_id = 0;
        let mut slots = Vec::new();
        let bios_slot = MemorySlot::anonymous(vm_fd.raw(), &mut slot_id, BIOS_START, BIOS_SIZE, 0)?;
        slots.push(MemorySlot::anonymous(
            vm_fd.raw(),
            &mut slot_id,
            EWRAM_START,
            EWRAM_SIZE,
            0,
        )?);
        let iwram_slot =
            MemorySlot::anonymous(vm_fd.raw(), &mut slot_id, IWRAM_START, IWRAM_SIZE, 0)?;
        let io_slot = MemorySlot::anonymous(
            vm_fd.raw(),
            &mut slot_id,
            IO_START,
            IO_SLOT_SIZE,
            sys::KVM_MEM_READONLY,
        )?;
        let palette_slot = MemorySlot::anonymous(
            vm_fd.raw(),
            &mut slot_id,
            PALETTE_START,
            PALETTE_SLOT_SIZE,
            0,
        )?;
        let vram_slot = MemorySlot::anonymous(vm_fd.raw(), &mut slot_id, VRAM_START, VRAM_SIZE, 0)?;
        let oam_slot =
            MemorySlot::anonymous(vm_fd.raw(), &mut slot_id, OAM_START, OAM_SLOT_SIZE, 0)?;
        slots.push(MemorySlot::rom(
            vm_fd.raw(),
            &mut slot_id,
            GAME_PAK_ROM_START,
            cartridge.rom(),
        )?);

        install_cache_bootstrap(&bios_slot.region, &iwram_slot.region);

        let shared = Arc::new(KvmSharedMemory {
            io: io_slot.region.clone_for_shared(),
            palette: palette_slot.region.clone_for_shared(),
            vram: vram_slot.region.clone_for_shared(),
            oam: oam_slot.region.clone_for_shared(),
            timers: Mutex::new(Timers::new()),
        });
        shared.write_io_u16(IO_START + 0x0130, 0x03ff);
        slots.push(bios_slot);
        slots.push(iwram_slot);
        slots.push(io_slot);
        slots.push(palette_slot);
        slots.push(vram_slot);
        slots.push(oam_slot);

        let vcpu_raw = unsafe { sys::ioctl_arg(vm_fd.raw(), sys::KVM_CREATE_VCPU, 0) };
        if vcpu_raw < 0 {
            return Err(last_os_error("KVM_CREATE_VCPU"));
        }
        let vcpu_fd = Fd(vcpu_raw);

        let mmap_size = unsafe { sys::ioctl_noarg(kvm_fd.raw(), sys::KVM_GET_VCPU_MMAP_SIZE) };
        if mmap_size <= 0 {
            return Err(last_os_error("KVM_GET_VCPU_MMAP_SIZE"));
        }
        let run = RunMapping::new(vcpu_fd.raw(), mmap_size as usize)?;

        let mut init = sys::KvmVcpuInit {
            target: 0,
            features: [0; 7],
        };
        let ret = unsafe {
            sys::ioctl_ptr(
                vm_fd.raw(),
                sys::KVM_ARM_PREFERRED_TARGET,
                (&mut init as *mut sys::KvmVcpuInit).cast(),
            )
        };
        if ret != 0 {
            return Err(last_os_error("KVM_ARM_PREFERRED_TARGET"));
        }
        init.features[0] = 1 << sys::KVM_ARM_VCPU_EL1_32BIT;
        let ret = unsafe {
            sys::ioctl_ptr(
                vcpu_fd.raw(),
                sys::KVM_ARM_VCPU_INIT,
                (&mut init as *mut sys::KvmVcpuInit).cast(),
            )
        };
        if ret != 0 {
            return Err(last_os_error("KVM_ARM_VCPU_INIT"));
        }

        set_one_reg_u64(vcpu_fd.raw(), sys::reg_arm64_core_pc(), BIOS_START as u64)?;

        Ok(Self {
            kvm_fd,
            vm_fd,
            vcpu_fd,
            run,
            _slots: slots,
            shared,
        })
    }

    pub fn shared_memory(&self) -> Arc<KvmSharedMemory> {
        Arc::clone(&self.shared)
    }

    pub fn run(mut self, stop: Arc<AtomicBool>) -> Result<(), String> {
        let _keep_fds_alive = (self.kvm_fd.raw(), self.vm_fd.raw());
        while !stop.load(Ordering::Relaxed) {
            let ret = unsafe { sys::ioctl_noarg(self.vcpu_fd.raw(), sys::KVM_RUN) };
            if ret != 0 {
                return Err(last_os_error("KVM_RUN"));
            }

            match self.run.exit_reason() {
                sys::KVM_EXIT_MMIO => self.handle_mmio(),
                sys::KVM_EXIT_EXCEPTION
                | sys::KVM_EXIT_FAIL_ENTRY
                | sys::KVM_EXIT_INTERNAL_ERROR
                | sys::KVM_EXIT_SHUTDOWN => {
                    return Err(format!(
                        "KVM stopped with exit reason {}",
                        self.run.exit_reason()
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn handle_mmio(&mut self) {
        let mmio = self.run.mmio();
        if mmio.phys_addr >= u64::from(IO_START)
            && mmio.phys_addr < u64::from(IO_START + IO_SIZE as u32)
        {
            let addr = mmio.phys_addr as u32;
            if mmio.is_write != 0 {
                trace_io_mmio("write", addr, mmio.len, &mmio.data);
                self.shared.mirror_io_write(addr, mmio.len, &mmio.data);
                self.shared.write_timer_registers_from_io(addr, mmio.len);
            } else {
                let len = mmio.len;
                self.shared
                    .copy_io_read(addr, len, &mut self.run.mmio_mut().data);
                trace_io_mmio("read", addr, len, &self.run.mmio().data);
            }
        } else if mmio.is_write == 0 {
            self.run.mmio_mut().data = [0; 8];
        }
    }
}

#[derive(Debug, Default)]
struct Timers {
    timers: [Timer; 4],
}

impl Timers {
    fn new() -> Self {
        Self::default()
    }

    fn write_register(&mut self, addr: u32, value: u16, io: &MemoryRegion) {
        let relative = addr - (IO_START + 0x0100);
        let timer_index = (relative / 4) as usize;
        if timer_index >= self.timers.len() {
            return;
        }

        if relative & 0x2 == 0 {
            self.timers[timer_index].reload = value;
        } else {
            let was_enabled = self.timers[timer_index].enabled();
            self.timers[timer_index].control = value;
            self.timers[timer_index].accumulated_cycles = 0;
            if !was_enabled && self.timers[timer_index].enabled() {
                self.timers[timer_index].counter = self.timers[timer_index].reload;
                write_timer_counter(io, timer_index, self.timers[timer_index].counter);
            }
        }
    }

    fn advance(&mut self, cycles: u32, io: &MemoryRegion) {
        let overflows = self.advance_timer(0, cycles, io);
        let overflows = self.advance_cascade_timer(1, overflows, io);
        let overflows = self.advance_cascade_timer(2, overflows, io);
        self.advance_cascade_timer(3, overflows, io);
    }

    fn advance_timer(&mut self, timer_index: usize, cycles: u32, io: &MemoryRegion) -> u32 {
        let timer = &mut self.timers[timer_index];
        if !timer.enabled() || timer.cascade() {
            return 0;
        }

        timer.accumulated_cycles += u64::from(cycles);
        let period = u64::from(timer.period_cycles());
        let ticks = (timer.accumulated_cycles / period) as u32;
        timer.accumulated_cycles %= period;
        self.add_ticks(timer_index, ticks, io)
    }

    fn advance_cascade_timer(&mut self, timer_index: usize, ticks: u32, io: &MemoryRegion) -> u32 {
        let timer = &self.timers[timer_index];
        if !timer.enabled() || !timer.cascade() {
            return 0;
        }
        self.add_ticks(timer_index, ticks, io)
    }

    fn add_ticks(&mut self, timer_index: usize, ticks: u32, io: &MemoryRegion) -> u32 {
        let mut overflows = 0;
        for _ in 0..ticks {
            let timer = &mut self.timers[timer_index];
            let (next, overflow) = timer.counter.overflowing_add(1);
            if overflow {
                timer.counter = timer.reload;
                overflows += 1;
            } else {
                timer.counter = next;
            }
        }
        write_timer_counter(io, timer_index, self.timers[timer_index].counter);
        trace_timer_counter(
            timer_index,
            self.timers[timer_index].counter,
            ticks,
            overflows,
        );
        overflows
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Timer {
    reload: u16,
    counter: u16,
    control: u16,
    accumulated_cycles: u64,
}

impl Timer {
    fn enabled(self) -> bool {
        self.control & (1 << 7) != 0
    }

    fn cascade(self) -> bool {
        self.control & (1 << 2) != 0
    }

    fn period_cycles(self) -> u32 {
        match self.control & 0x3 {
            0 => 1,
            1 => 64,
            2 => 256,
            3 => 1024,
            _ => unreachable!(),
        }
    }
}

fn write_timer_counter(io: &MemoryRegion, timer_index: usize, value: u16) {
    let offset = 0x0100 + timer_index * 4;
    let bytes = value.to_le_bytes();
    let io = io.as_mut_slice();
    io[offset] = bytes[0];
    io[offset + 1] = bytes[1];
    clean_dcache_area(io.as_mut_ptr().wrapping_add(offset), 2);
}

fn trace_timer_register_write(addr: u32, value: u16) {
    if trace_enabled("KGBA_TRACE_TIMER") {
        eprintln!(
            "kgba timer write addr={addr:#010x} value={value:#06x} kind={}",
            if (addr - (IO_START + 0x0100)) & 0x2 == 0 {
                "reload"
            } else {
                "control"
            }
        );
    }
}

fn trace_timer_counter(timer_index: usize, value: u16, ticks: u32, overflows: u32) {
    if !trace_enabled("KGBA_TRACE_TIMER") || ticks == 0 {
        return;
    }

    static COUNTER_LOGS: AtomicU64 = AtomicU64::new(0);
    let log_index = COUNTER_LOGS.fetch_add(1, Ordering::Relaxed);
    if log_index < 32 || log_index.is_multiple_of(1024) {
        eprintln!(
            "kgba timer advance timer={} value={} ticks={} overflows={}",
            timer_index, value, ticks, overflows
        );
    }
}

fn trace_io_mmio(kind: &str, addr: u32, len: u32, data: &[u8; 8]) {
    if !trace_enabled("KGBA_TRACE_MMIO") && !is_timer_register_access(addr, len) {
        return;
    }

    eprintln!(
        "kgba mmio {kind} addr={addr:#010x} len={} data={}",
        len,
        format_mmio_data(data, len)
    );
}

fn is_timer_register_access(addr: u32, len: u32) -> bool {
    let end = addr.saturating_add(len);
    addr < IO_START + 0x0110 && end > IO_START + 0x0100
}

fn format_mmio_data(data: &[u8; 8], len: u32) -> String {
    data[..len as usize]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn trace_enabled(name: &'static str) -> bool {
    static TIMER: OnceLock<bool> = OnceLock::new();
    static MMIO: OnceLock<bool> = OnceLock::new();

    match name {
        "KGBA_TRACE_TIMER" => *TIMER.get_or_init(|| env::var_os(name).is_some()),
        "KGBA_TRACE_MMIO" => *MMIO.get_or_init(|| env::var_os(name).is_some()),
        _ => false,
    }
}

fn install_cache_bootstrap(bios: &MemoryRegion, iwram: &MemoryRegion) {
    for (index, word) in BIOS_VECTOR_TABLE.iter().enumerate() {
        let offset = index * 4;
        bios.as_mut_slice()[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }

    for (index, word) in CACHE_BOOTSTRAP.iter().enumerate() {
        let offset = BIOS_RESET_HANDLER_OFFSET + index * 4;
        bios.as_mut_slice()[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }

    for (index, word) in BIOS_SWI_HANDLER.iter().enumerate() {
        let offset = BIOS_SWI_HANDLER_OFFSET + index * 4;
        bios.as_mut_slice()[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }

    clean_dcache_area(
        bios.ptr,
        BIOS_SWI_HANDLER_OFFSET + BIOS_SWI_HANDLER.len() * 4,
    );

    let table =
        &mut iwram.as_mut_slice()[PAGE_TABLE_IWRAM_OFFSET..PAGE_TABLE_IWRAM_OFFSET + 0x4000];
    for section in 0..4096u32 {
        let base = section * SECTION_SIZE;
        let entry = base | NORMAL_SHARED_WBWA_SECTION;
        let offset = section as usize * 4;
        table[offset..offset + 4].copy_from_slice(&entry.to_le_bytes());
    }
    clean_dcache_area(iwram.ptr_at(PAGE_TABLE_IWRAM_OFFSET), 0x4000);
}

fn set_one_reg_u64(vcpu_fd: RawFd, id: u64, mut value: u64) -> Result<(), String> {
    let mut reg = sys::KvmOneReg {
        id,
        addr: (&mut value as *mut u64) as u64,
    };
    let ret = unsafe {
        sys::ioctl_ptr(
            vcpu_fd,
            sys::KVM_SET_ONE_REG,
            (&mut reg as *mut sys::KvmOneReg).cast(),
        )
    };
    if ret != 0 {
        return Err(last_os_error("KVM_SET_ONE_REG"));
    }
    Ok(())
}

struct MemorySlot {
    region: MemoryRegion,
}

impl MemorySlot {
    fn anonymous(
        vm_fd: RawFd,
        next_slot: &mut u32,
        guest_addr: u32,
        size: usize,
        flags: u32,
    ) -> Result<Self, String> {
        let region = MemoryRegion::anonymous(size)?;
        set_user_memory_region(vm_fd, *next_slot, flags, guest_addr, &region)?;
        *next_slot += 1;
        Ok(Self { region })
    }

    fn rom(vm_fd: RawFd, next_slot: &mut u32, guest_addr: u32, rom: &[u8]) -> Result<Self, String> {
        let size = align_up(rom.len(), page_size()?);
        let region = MemoryRegion::anonymous(size)?;
        region.as_mut_slice()[..rom.len()].copy_from_slice(rom);
        clean_dcache_area(region.ptr, rom.len());
        set_user_memory_region(
            vm_fd,
            *next_slot,
            sys::KVM_MEM_READONLY,
            guest_addr,
            &region,
        )?;
        *next_slot += 1;
        Ok(Self { region })
    }
}

fn set_user_memory_region(
    vm_fd: RawFd,
    slot: u32,
    flags: u32,
    guest_addr: u32,
    region: &MemoryRegion,
) -> Result<(), String> {
    let mut kvm_region = sys::KvmUserspaceMemoryRegion {
        slot,
        flags,
        guest_phys_addr: u64::from(guest_addr),
        memory_size: region.len as u64,
        userspace_addr: region.ptr as u64,
    };
    let ret = unsafe {
        sys::ioctl_ptr(
            vm_fd,
            sys::KVM_SET_USER_MEMORY_REGION,
            (&mut kvm_region as *mut sys::KvmUserspaceMemoryRegion).cast(),
        )
    };
    if ret != 0 {
        return Err(last_os_error("KVM_SET_USER_MEMORY_REGION"));
    }
    Ok(())
}

#[derive(Debug)]
struct MemoryRegion {
    ptr: *mut u8,
    len: usize,
    owned: bool,
}

unsafe impl Send for MemoryRegion {}
unsafe impl Sync for MemoryRegion {}

impl MemoryRegion {
    fn anonymous(len: usize) -> Result<Self, String> {
        let ptr = unsafe {
            sys::mmap(
                ptr::null_mut(),
                len,
                sys::PROT_READ | sys::PROT_WRITE,
                sys::MAP_SHARED | sys::MAP_ANONYMOUS | sys::MAP_NORESERVE,
                -1,
                0,
            )
        };
        if ptr == sys::MAP_FAILED {
            return Err(last_os_error("mmap"));
        }
        Ok(Self {
            ptr: ptr.cast(),
            len,
            owned: true,
        })
    }

    fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    fn as_mut_slice(&self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    fn ptr_at(&self, offset: usize) -> *mut u8 {
        unsafe { self.ptr.add(offset) }
    }

    fn clone_for_shared(&self) -> Self {
        Self {
            ptr: self.ptr,
            len: self.len,
            owned: false,
        }
    }
}

impl Drop for MemoryRegion {
    fn drop(&mut self) {
        // Shared VRAM/IO views can outlive the KVM thread long enough for the
        // SDL thread to report/display the last frame. Let the OS reclaim these
        // VM mappings when the process exits.
        let _ = self.owned;
    }
}

struct RunMapping {
    ptr: *mut sys::KvmRun,
    len: usize,
}

unsafe impl Send for RunMapping {}

impl RunMapping {
    fn new(vcpu_fd: RawFd, len: usize) -> Result<Self, String> {
        let ptr = unsafe {
            sys::mmap(
                ptr::null_mut(),
                len,
                sys::PROT_READ | sys::PROT_WRITE,
                sys::MAP_SHARED,
                vcpu_fd,
                0,
            )
        };
        if ptr == sys::MAP_FAILED {
            return Err(last_os_error("mmap kvm_run"));
        }
        Ok(Self {
            ptr: ptr.cast(),
            len,
        })
    }

    fn exit_reason(&self) -> u32 {
        unsafe { (*self.ptr).exit_reason }
    }

    fn mmio(&self) -> &sys::KvmMmio {
        unsafe { &(*self.ptr).mmio }
    }

    fn mmio_mut(&mut self) -> &mut sys::KvmMmio {
        unsafe { &mut (*self.ptr).mmio }
    }
}

impl Drop for RunMapping {
    fn drop(&mut self) {
        unsafe {
            sys::munmap(self.ptr.cast(), self.len);
        }
    }
}

struct Fd(RawFd);

impl Fd {
    fn open(path: &str) -> Result<Self, String> {
        let path = std::ffi::CString::new(path).map_err(|err| err.to_string())?;
        let fd = unsafe { sys::open(path.as_ptr(), sys::O_RDWR) };
        if fd < 0 {
            return Err(last_os_error("open /dev/kvm"));
        }
        Ok(Self(fd))
    }

    fn raw(&self) -> RawFd {
        self.0
    }
}

impl Drop for Fd {
    fn drop(&mut self) {
        unsafe {
            sys::close(self.0);
        }
    }
}

fn page_size() -> Result<usize, String> {
    let size = unsafe { sys::sysconf(sys::_SC_PAGESIZE) };
    if size <= 0 {
        return Err(last_os_error("sysconf(_SC_PAGESIZE)"));
    }
    Ok(size as usize)
}

fn align_up(value: usize, align: usize) -> usize {
    value.div_ceil(align) * align
}

fn last_os_error(context: &str) -> String {
    format!("{context}: {}", std::io::Error::last_os_error())
}

#[cfg(target_arch = "aarch64")]
fn clean_dcache_area(ptr: *mut u8, len: usize) {
    use std::arch::asm;

    if ptr.is_null() || len == 0 {
        return;
    }

    /* */
    let start = ptr as usize & !63;
    let end = (ptr as usize + len + 63) & !63;
    let mut addr = start;
    while addr < end {
        unsafe {
            asm!("dc cvac, {addr}", addr = in(reg) addr, options(nostack, preserves_flags));
        }
        addr += 64;
    }
    unsafe {
        asm!("dsb sy", options(nostack, preserves_flags));
    }
}

#[cfg(not(target_arch = "aarch64"))]
fn clean_dcache_area(_ptr: *mut u8, _len: usize) {}

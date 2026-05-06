mod sys;

use std::{
    os::fd::RawFd,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
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

#[derive(Debug)]
pub struct KvmSharedMemory {
    io: MemoryRegion,
    vram: MemoryRegion,
}

unsafe impl Send for KvmSharedMemory {}
unsafe impl Sync for KvmSharedMemory {}

impl KvmSharedMemory {
    pub fn set_vcount(&self, value: u16) {
        self.write_io_u16(VCOUNT, value);
    }

    pub fn render_mode3(&self) -> FrameBuffer {
        let mut ppu = Ppu::new();
        ppu.write_dispcnt(self.read_io_u16(DISPCNT));
        ppu.render_mode3(self.vram.as_slice())
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
        slots.push(MemorySlot::anonymous(
            vm_fd.raw(),
            &mut slot_id,
            BIOS_START,
            BIOS_SIZE,
            0,
        )?);
        slots.push(MemorySlot::anonymous(
            vm_fd.raw(),
            &mut slot_id,
            EWRAM_START,
            EWRAM_SIZE,
            0,
        )?);
        slots.push(MemorySlot::anonymous(
            vm_fd.raw(),
            &mut slot_id,
            IWRAM_START,
            IWRAM_SIZE,
            0,
        )?);
        let io_slot = MemorySlot::anonymous(
            vm_fd.raw(),
            &mut slot_id,
            IO_START,
            IO_SLOT_SIZE,
            sys::KVM_MEM_READONLY,
        )?;
        slots.push(MemorySlot::anonymous(
            vm_fd.raw(),
            &mut slot_id,
            PALETTE_START,
            PALETTE_SLOT_SIZE,
            0,
        )?);
        let vram_slot = MemorySlot::anonymous(vm_fd.raw(), &mut slot_id, VRAM_START, VRAM_SIZE, 0)?;
        slots.push(MemorySlot::anonymous(
            vm_fd.raw(),
            &mut slot_id,
            OAM_START,
            OAM_SLOT_SIZE,
            0,
        )?);
        slots.push(MemorySlot::rom(
            vm_fd.raw(),
            &mut slot_id,
            GAME_PAK_ROM_START,
            cartridge.rom(),
        )?);

        let shared = Arc::new(KvmSharedMemory {
            io: io_slot.region.clone_for_shared(),
            vram: vram_slot.region.clone_for_shared(),
        });
        slots.push(io_slot);
        slots.push(vram_slot);

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

        set_one_reg_u64(
            vcpu_fd.raw(),
            sys::reg_arm64_core_pc(),
            GAME_PAK_ROM_START as u64,
        )?;

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
            && mmio.is_write != 0
        {
            self.shared
                .mirror_io_write(mmio.phys_addr as u32, mmio.len, &mmio.data);
        } else if mmio.is_write == 0 {
            self.run.mmio_mut().data = [0; 8];
        }
    }
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

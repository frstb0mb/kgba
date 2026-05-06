mod bootstrap;
mod fd;
mod memory;
mod regs;
mod run;
mod shared_memory;
mod sys;
mod timers;
mod trace;
mod util;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::gba::{
    cartridge::Cartridge,
    memory_map::{
        BIOS_SIZE, BIOS_START, EWRAM_SIZE, EWRAM_START, GAME_PAK_ROM_START, IO_SIZE, IO_START,
        IWRAM_SIZE, IWRAM_START, KEYINPUT, OAM_START, PALETTE_START, VRAM_SIZE, VRAM_START,
    },
};

use self::{
    bootstrap::install_bios_and_cache_bootstrap, fd::Fd, memory::MemorySlot, regs::set_one_reg_u64,
    run::RunMapping, trace::trace_io_mmio, util::last_os_error,
};

pub use self::shared_memory::KvmSharedMemory;

const IO_SLOT_SIZE: usize = 0x1000;
const PALETTE_SLOT_SIZE: usize = 0x1000;
const OAM_SLOT_SIZE: usize = 0x1000;

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
        let vm_fd = Fd::from_raw(vm_raw);

        let mut slot_id = 0;
        let mut slots = Vec::new();
        let bios_slot = MemorySlot::anonymous(vm_fd.raw(), &mut slot_id, BIOS_START, BIOS_SIZE, 0)?;
        let ewram_slot =
            MemorySlot::anonymous(vm_fd.raw(), &mut slot_id, EWRAM_START, EWRAM_SIZE, 0)?;
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

        install_bios_and_cache_bootstrap(&bios_slot.region, &iwram_slot.region);

        let shared = Arc::new(KvmSharedMemory::new(
            ewram_slot.region.clone_for_shared(),
            iwram_slot.region.clone_for_shared(),
            io_slot.region.clone_for_shared(),
            palette_slot.region.clone_for_shared(),
            vram_slot.region.clone_for_shared(),
            oam_slot.region.clone_for_shared(),
            cartridge.rom(),
            vm_fd.raw(),
        ));
        shared.write_io_u16(KEYINPUT, 0x03ff);
        slots.push(bios_slot);
        slots.push(ewram_slot);
        slots.push(iwram_slot);
        slots.push(io_slot);
        slots.push(palette_slot);
        slots.push(vram_slot);
        slots.push(oam_slot);

        let vcpu_raw = unsafe { sys::ioctl_arg(vm_fd.raw(), sys::KVM_CREATE_VCPU, 0) };
        if vcpu_raw < 0 {
            return Err(last_os_error("KVM_CREATE_VCPU"));
        }
        let vcpu_fd = Fd::from_raw(vcpu_raw);

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
                self.shared.run_immediate_dma_for_io_write(addr, mmio.len);
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

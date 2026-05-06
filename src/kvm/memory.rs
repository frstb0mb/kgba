use std::{os::fd::RawFd, ptr};

use super::{
    sys,
    util::{align_up, clean_dcache_area, last_os_error, page_size},
};

pub struct MemorySlot {
    pub region: MemoryRegion,
}

impl MemorySlot {
    pub fn anonymous(
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

    pub fn rom(
        vm_fd: RawFd,
        next_slot: &mut u32,
        guest_addr: u32,
        rom: &[u8],
    ) -> Result<Self, String> {
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
pub struct MemoryRegion {
    pub ptr: *mut u8,
    len: usize,
    owned: bool,
}

unsafe impl Send for MemoryRegion {}
unsafe impl Sync for MemoryRegion {}

impl MemoryRegion {
    pub fn anonymous(len: usize) -> Result<Self, String> {
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

    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    pub fn as_mut_slice(&self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    pub fn ptr_at(&self, offset: usize) -> *mut u8 {
        unsafe { self.ptr.add(offset) }
    }

    pub fn clone_for_shared(&self) -> Self {
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

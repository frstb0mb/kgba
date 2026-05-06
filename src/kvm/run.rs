use std::{os::fd::RawFd, ptr};

use super::{sys, util::last_os_error};

pub struct RunMapping {
    ptr: *mut sys::KvmRun,
    len: usize,
}

unsafe impl Send for RunMapping {}

impl RunMapping {
    pub fn new(vcpu_fd: RawFd, len: usize) -> Result<Self, String> {
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

    pub fn exit_reason(&self) -> u32 {
        unsafe { (*self.ptr).exit_reason }
    }

    pub fn mmio(&self) -> &sys::KvmMmio {
        unsafe { &(*self.ptr).mmio }
    }

    pub fn mmio_mut(&mut self) -> &mut sys::KvmMmio {
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

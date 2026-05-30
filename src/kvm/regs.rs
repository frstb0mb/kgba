use std::os::fd::RawFd;

use super::{sys, util::last_os_error};

pub fn get_one_reg_u64(vcpu_fd: RawFd, id: u64) -> Result<u64, String> {
    let mut value = 0u64;
    let mut reg = sys::KvmOneReg {
        id,
        addr: (&mut value as *mut u64) as u64,
    };
    let ret = unsafe {
        sys::ioctl_ptr(
            vcpu_fd,
            sys::KVM_GET_ONE_REG,
            (&mut reg as *mut sys::KvmOneReg).cast(),
        )
    };
    if ret != 0 {
        return Err(last_os_error("KVM_GET_ONE_REG"));
    }
    Ok(value)
}

pub fn set_one_reg_u64(vcpu_fd: RawFd, id: u64, mut value: u64) -> Result<(), String> {
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

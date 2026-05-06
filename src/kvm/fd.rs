use std::os::fd::RawFd;

use super::{sys, util::last_os_error};

pub struct Fd(RawFd);

impl Fd {
    pub fn open(path: &str) -> Result<Self, String> {
        let path = std::ffi::CString::new(path).map_err(|err| err.to_string())?;
        let fd = unsafe { sys::open(path.as_ptr(), sys::O_RDWR) };
        if fd < 0 {
            return Err(last_os_error("open /dev/kvm"));
        }
        Ok(Self(fd))
    }

    pub fn from_raw(fd: RawFd) -> Self {
        Self(fd)
    }

    pub fn raw(&self) -> RawFd {
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

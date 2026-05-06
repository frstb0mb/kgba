pub fn page_size() -> Result<usize, String> {
    let size = unsafe { super::sys::sysconf(super::sys::_SC_PAGESIZE) };
    if size <= 0 {
        return Err(last_os_error("sysconf(_SC_PAGESIZE)"));
    }
    Ok(size as usize)
}

pub fn align_up(value: usize, align: usize) -> usize {
    value.div_ceil(align) * align
}

pub fn last_os_error(context: &str) -> String {
    format!("{context}: {}", std::io::Error::last_os_error())
}

#[cfg(target_arch = "aarch64")]
pub fn clean_dcache_area(ptr: *mut u8, len: usize) {
    use std::arch::asm;

    if ptr.is_null() || len == 0 {
        return;
    }

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
pub fn clean_dcache_area(_ptr: *mut u8, _len: usize) {}

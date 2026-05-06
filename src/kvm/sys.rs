use std::{
    ffi::{c_char, c_int, c_ulong, c_void},
    os::fd::RawFd,
};

pub const KVM_API_VERSION: c_int = 12;
pub const KVM_MEM_READONLY: u32 = 1 << 1;

pub const KVM_EXIT_EXCEPTION: u32 = 1;
pub const KVM_EXIT_MMIO: u32 = 6;
pub const KVM_EXIT_SHUTDOWN: u32 = 8;
pub const KVM_EXIT_FAIL_ENTRY: u32 = 9;
pub const KVM_EXIT_INTERNAL_ERROR: u32 = 17;

pub const KVM_ARM_VCPU_EL1_32BIT: u32 = 1;

const KVMIO: u32 = 0xae;
const IOC_NONE: u32 = 0;
const IOC_WRITE: u32 = 1;
const IOC_READ: u32 = 2;
const IOC_NRSHIFT: u32 = 0;
const IOC_TYPESHIFT: u32 = 8;
const IOC_SIZESHIFT: u32 = 16;
const IOC_DIRSHIFT: u32 = 30;

const fn ioc(dir: u32, ty: u32, nr: u32, size: usize) -> c_ulong {
    ((dir << IOC_DIRSHIFT)
        | (ty << IOC_TYPESHIFT)
        | (nr << IOC_NRSHIFT)
        | ((size as u32) << IOC_SIZESHIFT)) as c_ulong
}

const fn io(ty: u32, nr: u32) -> c_ulong {
    ioc(IOC_NONE, ty, nr, 0)
}

const fn iow<T>(ty: u32, nr: u32) -> c_ulong {
    ioc(IOC_WRITE, ty, nr, size_of::<T>())
}

const fn ior<T>(ty: u32, nr: u32) -> c_ulong {
    ioc(IOC_READ, ty, nr, size_of::<T>())
}

pub const KVM_GET_API_VERSION: c_ulong = io(KVMIO, 0x00);
pub const KVM_CREATE_VM: c_ulong = io(KVMIO, 0x01);
pub const KVM_GET_VCPU_MMAP_SIZE: c_ulong = io(KVMIO, 0x04);
pub const KVM_CREATE_VCPU: c_ulong = io(KVMIO, 0x41);
pub const KVM_SET_USER_MEMORY_REGION: c_ulong = iow::<KvmUserspaceMemoryRegion>(KVMIO, 0x46);
pub const KVM_RUN: c_ulong = io(KVMIO, 0x80);
pub const KVM_SET_ONE_REG: c_ulong = iow::<KvmOneReg>(KVMIO, 0xac);
pub const KVM_ARM_VCPU_INIT: c_ulong = iow::<KvmVcpuInit>(KVMIO, 0xae);
pub const KVM_ARM_PREFERRED_TARGET: c_ulong = ior::<KvmVcpuInit>(KVMIO, 0xaf);

const KVM_REG_ARM64: u64 = 0x6000_0000_0000_0000;
const KVM_REG_SIZE_U64: u64 = 0x0030_0000_0000_0000;
const KVM_REG_ARM_CORE: u64 = 0x0010 << 16;
const KVM_REG_ARM_CORE_PC: u64 = 64;

pub const O_RDWR: c_int = 2;
pub const PROT_READ: c_int = 0x1;
pub const PROT_WRITE: c_int = 0x2;
pub const MAP_SHARED: c_int = 0x01;
pub const MAP_ANONYMOUS: c_int = 0x20;
pub const MAP_NORESERVE: c_int = 0x4000;
pub const MAP_FAILED: *mut c_void = !0usize as *mut c_void;
pub const _SC_PAGESIZE: c_int = 30;

#[repr(C)]
pub struct KvmUserspaceMemoryRegion {
    pub slot: u32,
    pub flags: u32,
    pub guest_phys_addr: u64,
    pub memory_size: u64,
    pub userspace_addr: u64,
}

#[repr(C)]
pub struct KvmVcpuInit {
    pub target: u32,
    pub features: [u32; 7],
}

#[repr(C)]
pub struct KvmOneReg {
    pub id: u64,
    pub addr: u64,
}

#[repr(C)]
pub struct KvmMmio {
    pub phys_addr: u64,
    pub data: [u8; 8],
    pub len: u32,
    pub is_write: u8,
}

#[repr(C)]
pub struct KvmRun {
    pub request_interrupt_window: u8,
    pub immediate_exit: u8,
    pub padding1: [u8; 6],
    pub exit_reason: u32,
    pub ready_for_interrupt_injection: u8,
    pub if_flag: u8,
    pub flags: u16,
    pub cr8: u64,
    pub apic_base: u64,
    pub mmio: KvmMmio,
}

pub const fn reg_arm64_core_pc() -> u64 {
    KVM_REG_ARM64 | KVM_REG_SIZE_U64 | KVM_REG_ARM_CORE | KVM_REG_ARM_CORE_PC
}

unsafe extern "C" {
    pub fn open(pathname: *const c_char, flags: c_int, ...) -> c_int;
    pub fn close(fd: c_int) -> c_int;
    pub fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    pub fn mmap(
        addr: *mut c_void,
        length: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        offset: isize,
    ) -> *mut c_void;
    pub fn munmap(addr: *mut c_void, length: usize) -> c_int;
    pub fn sysconf(name: c_int) -> isize;
}

pub unsafe fn ioctl_noarg(fd: RawFd, request: c_ulong) -> c_int {
    unsafe { ioctl(fd, request, 0) }
}

pub unsafe fn ioctl_arg(fd: RawFd, request: c_ulong, arg: c_ulong) -> c_int {
    unsafe { ioctl(fd, request, arg) }
}

pub unsafe fn ioctl_ptr(fd: RawFd, request: c_ulong, arg: *mut c_void) -> c_int {
    unsafe { ioctl(fd, request, arg) }
}

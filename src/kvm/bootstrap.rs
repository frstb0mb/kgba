use crate::gba::memory_map::{
    EWRAM_START, GAME_PAK_ROM_START, IO_START, IWRAM_START, OAM_START, PALETTE_START, VRAM_START,
};

use super::{memory::MemoryRegion, util::clean_dcache_area};

pub const FAST_MEM_START: u32 = 0x0f00_0000;
pub const FAST_MEM_SIZE: usize = 0x0001_0000;
pub const NORMAL_L1_ADDR: u32 = FAST_MEM_START;
#[allow(dead_code)]
pub const HBLANK_L1_ADDR: u32 = FAST_MEM_START + 0x4000;
pub const HBLANK_IO_L2_ADDR: u32 = FAST_MEM_START + 0x8000;
pub const SHADOW_IO_ADDR: u32 = FAST_MEM_START + 0x9000;
pub const FAST_CTRL_ADDR: u32 = FAST_MEM_START + 0xa000;
pub const FAST_EXIT_ADDR: u32 = IO_START + 0x03fc;

const NORMAL_L1_OFFSET: usize = 0;
const HBLANK_L1_OFFSET: usize = 0x4000;
const HBLANK_IO_L2_OFFSET: usize = 0x8000;
const SECTION_SIZE: u32 = 0x0010_0000;
const TTBR0_INNER_SHAREABLE_WBWA: u32 = (1 << 6) | (1 << 3) | (1 << 1);
const TTBR0_VALUE: u32 = NORMAL_L1_ADDR | TTBR0_INNER_SHAREABLE_WBWA;
const KVM_RESET_VECTOR_OFFSET: usize = 0;
const KVM_RESET_HANDLER_OFFSET: usize = 0x300;

const SECTION_DESCRIPTOR: u32 = 0b10;
const SECTION_BUFFERABLE: u32 = 1 << 2;
const SECTION_CACHEABLE: u32 = 1 << 3;
const SECTION_AP_FULL_ACCESS: u32 = 0b11 << 10;
const SECTION_TEX_WRITE_BACK_WRITE_ALLOCATE: u32 = 0b001 << 12;
const SECTION_SHAREABLE: u32 = 1 << 16;
const PAGE_DESCRIPTOR_COARSE: u32 = 0b01;
const SMALL_PAGE_DESCRIPTOR: u32 = 0b10;
const SMALL_PAGE_BUFFERABLE: u32 = 1 << 2;
const SMALL_PAGE_CACHEABLE: u32 = 1 << 3;
const SMALL_PAGE_AP_FULL_ACCESS: u32 = 0b11 << 4;
const SMALL_PAGE_TEX_WRITE_BACK_WRITE_ALLOCATE: u32 = 0b001 << 6;
const L1_DOMAIN_CLIENT: u32 = 0 << 5;

const NORMAL_SHARED_WBWA_SECTION: u32 = SECTION_DESCRIPTOR
    | SECTION_BUFFERABLE
    | SECTION_CACHEABLE
    | SECTION_AP_FULL_ACCESS
    | SECTION_TEX_WRITE_BACK_WRITE_ALLOCATE
    | SECTION_SHAREABLE;

const NORMAL_SHARED_WRITE_THROUGH_SECTION: u32 =
    SECTION_DESCRIPTOR | SECTION_CACHEABLE | SECTION_AP_FULL_ACCESS | SECTION_SHAREABLE;

const HBLANK_IO_L2_DESCRIPTOR: u32 =
    (HBLANK_IO_L2_ADDR & 0xffff_fc00) | L1_DOMAIN_CLIENT | PAGE_DESCRIPTOR_COARSE;

const SHADOW_IO_SMALL_PAGE: u32 = (SHADOW_IO_ADDR & 0xffff_f000)
    | SMALL_PAGE_BUFFERABLE
    | SMALL_PAGE_CACHEABLE
    | SMALL_PAGE_AP_FULL_ACCESS
    | SMALL_PAGE_TEX_WRITE_BACK_WRITE_ALLOCATE
    | SMALL_PAGE_DESCRIPTOR;

const CACHE_BOOTSTRAP: [u32; 19] = [
    0xe3a0_0001, // mov r0, #1 ; normal ASID
    0xee0d_0f30, // mcr p15, 0, r0, c13, c0, 1 ; CONTEXTIDR
    0xe59f_002c, // ldr r0, [pc, #0x2c] ; TTBR0
    0xee02_0f10, // mcr p15, 0, r0, c2, c0, 0
    0xe3e0_0000, // mvn r0, #0 ; DACR all manager
    0xee03_0f10, // mcr p15, 0, r0, c3, c0, 0
    0xee11_0f10, // mrc p15, 0, r0, c1, c0, 0 ; SCTLR
    0xe59f_101c, // ldr r1, [pc, #0x1c] ; M | C | I
    0xe180_0001, // orr r0, r0, r1
    0xee01_0f10, // mcr p15, 0, r0, c1, c0, 0
    0xf57f_f04f, // dsb sy
    0xf57f_f06f, // isb sy
    0xe59f_d00c, // ldr sp, [pc, #0x0c] ; SVC stack
    0xe59f_000c, // ldr r0, [pc, #0x0c] ; ROM entry
    0xe12f_ff10, // bx r0
    TTBR0_VALUE,
    0x0000_1005, // SCTLR.M | SCTLR.C | SCTLR.I
    0x0300_7fe0, // SVC stack, below REG_IRQ_WAITFLAGS/INT_VECTOR
    GAME_PAK_ROM_START,
];

const KVM_RESET_VECTOR: u32 = 0xea00_00be; // reset -> 0x300

pub fn install_bios_and_cache_bootstrap(
    bios: &MemoryRegion,
    _iwram: &MemoryRegion,
    fast_memory: &MemoryRegion,
) {
    bios.as_mut_slice()
        .copy_from_slice(&kgba_bios::DEFAULT_BIOS_IMAGE);
    bios.as_mut_slice()[KVM_RESET_VECTOR_OFFSET..KVM_RESET_VECTOR_OFFSET + 4]
        .copy_from_slice(&KVM_RESET_VECTOR.to_le_bytes());

    for (index, word) in CACHE_BOOTSTRAP.iter().enumerate() {
        let offset = KVM_RESET_HANDLER_OFFSET + index * 4;
        bios.as_mut_slice()[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }

    clean_dcache_area(bios.ptr, kgba_bios::BIOS_SIZE);

    let fast = fast_memory.as_mut_slice();
    fast.fill(0);

    for section in 0..4096u32 {
        let base = section * SECTION_SIZE;
        let attrs = if matches!(
            base,
            EWRAM_START | IWRAM_START | PALETTE_START | VRAM_START | OAM_START
        ) {
            NORMAL_SHARED_WRITE_THROUGH_SECTION
        } else {
            NORMAL_SHARED_WBWA_SECTION
        };
        let entry = base | attrs;
        let offset = section as usize * 4;
        fast[NORMAL_L1_OFFSET + offset..NORMAL_L1_OFFSET + offset + 4]
            .copy_from_slice(&entry.to_le_bytes());
        fast[HBLANK_L1_OFFSET + offset..HBLANK_L1_OFFSET + offset + 4]
            .copy_from_slice(&entry.to_le_bytes());
    }

    let io_l1_offset = ((IO_START / SECTION_SIZE) as usize) * 4;
    fast[HBLANK_L1_OFFSET + io_l1_offset..HBLANK_L1_OFFSET + io_l1_offset + 4]
        .copy_from_slice(&HBLANK_IO_L2_DESCRIPTOR.to_le_bytes());
    fast[HBLANK_IO_L2_OFFSET..HBLANK_IO_L2_OFFSET + 4]
        .copy_from_slice(&SHADOW_IO_SMALL_PAGE.to_le_bytes());

    clean_dcache_area(fast_memory.ptr, FAST_MEM_SIZE);
}

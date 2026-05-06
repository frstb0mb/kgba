use crate::gba::memory_map::{GAME_PAK_ROM_START, IWRAM_START};

use super::{memory::MemoryRegion, util::clean_dcache_area};

const PAGE_TABLE_GUEST_ADDR: u32 = IWRAM_START + 0x4000;
const PAGE_TABLE_IWRAM_OFFSET: usize = 0x4000;
const SECTION_SIZE: u32 = 0x0010_0000;
const TTBR0_INNER_SHAREABLE_WBWA: u32 = (1 << 6) | (1 << 3) | (1 << 1);
const TTBR0_VALUE: u32 = PAGE_TABLE_GUEST_ADDR | TTBR0_INNER_SHAREABLE_WBWA;
const KVM_RESET_VECTOR_OFFSET: usize = 0;
const KVM_RESET_HANDLER_OFFSET: usize = 0x40;

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

const KVM_RESET_VECTOR: u32 = 0xea00_000e; // reset -> 0x40

pub fn install_bios_and_cache_bootstrap(bios: &MemoryRegion, iwram: &MemoryRegion) {
    bios.as_mut_slice()
        .copy_from_slice(&kgba_bios::DEFAULT_BIOS_IMAGE);
    bios.as_mut_slice()[KVM_RESET_VECTOR_OFFSET..KVM_RESET_VECTOR_OFFSET + 4]
        .copy_from_slice(&KVM_RESET_VECTOR.to_le_bytes());

    for (index, word) in CACHE_BOOTSTRAP.iter().enumerate() {
        let offset = KVM_RESET_HANDLER_OFFSET + index * 4;
        bios.as_mut_slice()[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }

    clean_dcache_area(bios.ptr, kgba_bios::BIOS_SIZE);

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

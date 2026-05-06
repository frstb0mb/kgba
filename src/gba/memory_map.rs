pub const BIOS_START: u32 = 0x0000_0000;
pub const BIOS_SIZE: usize = 0x4000;

pub const EWRAM_START: u32 = 0x0200_0000;
pub const EWRAM_SIZE: usize = 0x40000;

pub const IWRAM_START: u32 = 0x0300_0000;
pub const IWRAM_SIZE: usize = 0x8000;

pub const IO_START: u32 = 0x0400_0000;
pub const IO_SIZE: usize = 0x400;

pub const PALETTE_START: u32 = 0x0500_0000;
pub const PALETTE_SIZE: usize = 0x400;

pub const VRAM_START: u32 = 0x0600_0000;
pub const VRAM_SIZE: usize = 0x18000;

pub const OAM_START: u32 = 0x0700_0000;
pub const OAM_SIZE: usize = 0x400;

pub const GAME_PAK_ROM_START: u32 = 0x0800_0000;

pub const DISPCNT: u32 = IO_START;
pub const DISPSTAT: u32 = IO_START + 0x0004;
pub const VCOUNT: u32 = IO_START + 0x0006;

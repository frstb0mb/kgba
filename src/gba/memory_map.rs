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
pub const BG0CNT: u32 = IO_START + 0x0008;
pub const BG1CNT: u32 = IO_START + 0x000a;
pub const BG2CNT: u32 = IO_START + 0x000c;
pub const BG3CNT: u32 = IO_START + 0x000e;
pub const BG0HOFS: u32 = IO_START + 0x0010;
pub const BG0VOFS: u32 = IO_START + 0x0012;
pub const BG1HOFS: u32 = IO_START + 0x0014;
pub const BG1VOFS: u32 = IO_START + 0x0016;
pub const BG2HOFS: u32 = IO_START + 0x0018;
pub const BG2VOFS: u32 = IO_START + 0x001a;
pub const BG3HOFS: u32 = IO_START + 0x001c;
pub const BG3VOFS: u32 = IO_START + 0x001e;
pub const WIN0H: u32 = IO_START + 0x0040;
pub const WIN1H: u32 = IO_START + 0x0042;
pub const WIN0V: u32 = IO_START + 0x0044;
pub const WIN1V: u32 = IO_START + 0x0046;
pub const WININ: u32 = IO_START + 0x0048;
pub const WINOUT: u32 = IO_START + 0x004a;
pub const MOSAIC: u32 = IO_START + 0x004c;
pub const BLDCNT: u32 = IO_START + 0x0050;
pub const BLDALPHA: u32 = IO_START + 0x0052;
pub const BLDY: u32 = IO_START + 0x0054;
pub const DMA0SAD: u32 = IO_START + 0x00b0;
pub const DMA0DAD: u32 = IO_START + 0x00b4;
pub const DMA0CNT: u32 = IO_START + 0x00b8;
pub const DMA1SAD: u32 = IO_START + 0x00bc;
pub const DMA1DAD: u32 = IO_START + 0x00c0;
pub const DMA1CNT: u32 = IO_START + 0x00c4;
pub const DMA2SAD: u32 = IO_START + 0x00c8;
pub const DMA2DAD: u32 = IO_START + 0x00cc;
pub const DMA2CNT: u32 = IO_START + 0x00d0;
pub const DMA3SAD: u32 = IO_START + 0x00d4;
pub const DMA3DAD: u32 = IO_START + 0x00d8;
pub const DMA3CNT: u32 = IO_START + 0x00dc;
pub const KEYINPUT: u32 = IO_START + 0x0130;
pub const IE: u32 = IO_START + 0x0200;
pub const IF: u32 = IO_START + 0x0202;
pub const IME: u32 = IO_START + 0x0208;

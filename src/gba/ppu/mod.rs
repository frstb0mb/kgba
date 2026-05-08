pub const WIDTH: usize = 240;
pub const HEIGHT: usize = 160;
pub const VISIBLE_SCANLINES: u16 = 160;
pub const TOTAL_SCANLINES: u16 = 228;

pub const MODE_0: u16 = 0x0000;
pub const MODE_3: u16 = 0x0003;
pub const MODE_4: u16 = 0x0004;
pub const MODE_5: u16 = 0x0005;
pub const DISPCNT_MODE_MASK: u16 = 0x0007;
pub const BACKBUFFER: u16 = 1 << 4;
pub const OBJ_1D_MAPPING: u16 = 1 << 6;
pub const BG0_ENABLE: u16 = 1 << 8;
pub const BG1_ENABLE: u16 = 1 << 9;
pub const BG2_ENABLE: u16 = 1 << 10;
pub const BG3_ENABLE: u16 = 1 << 11;
pub const OBJ_ENABLE: u16 = 1 << 12;
pub const WIN0_ENABLE: u16 = 1 << 13;
pub const WIN1_ENABLE: u16 = 1 << 14;
pub const OBJ_WIN_ENABLE: u16 = 1 << 15;

mod registers;
mod renderer;

pub use registers::Ppu;
pub use renderer::{FrameBuffer, bgr555_to_argb8888};

const DISPSTAT_VBLANK: u16 = 1 << 0;
const DISPSTAT_HBLANK: u16 = 1 << 1;
const DISPSTAT_VCOUNT: u16 = 1 << 2;
const DISPSTAT_IRQ_WRITABLE_MASK: u16 = 0x0038;
const DISPSTAT_VCOUNT_SETTING_MASK: u16 = 0xff00;

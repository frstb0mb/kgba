pub const WIDTH: usize = 240;
pub const HEIGHT: usize = 160;
pub const VISIBLE_SCANLINES: u16 = 160;
pub const TOTAL_SCANLINES: u16 = 228;

pub const MODE_3: u16 = 0x0003;
pub const MODE_4: u16 = 0x0004;
pub const DISPCNT_MODE_MASK: u16 = 0x0007;
pub const BACKBUFFER: u16 = 1 << 4;
pub const BG2_ENABLE: u16 = 1 << 10;

const DISPSTAT_VBLANK: u16 = 1 << 0;
const DISPSTAT_HBLANK: u16 = 1 << 1;
const DISPSTAT_VCOUNT: u16 = 1 << 2;
const DISPSTAT_IRQ_WRITABLE_MASK: u16 = 0x0038;
const DISPSTAT_VCOUNT_SETTING_MASK: u16 = 0xff00;

pub type FrameBuffer = Vec<u32>;

#[derive(Debug, Default)]
pub struct Ppu {
    dispcnt: u16,
    dispstat: u16,
    vcount: u16,
}

impl Ppu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn dispcnt(&self) -> u16 {
        self.dispcnt
    }

    pub fn dispstat(&self) -> u16 {
        let mut value = self.dispstat
            & (DISPSTAT_HBLANK | DISPSTAT_IRQ_WRITABLE_MASK | DISPSTAT_VCOUNT_SETTING_MASK);
        if self.vcount >= VISIBLE_SCANLINES {
            value |= DISPSTAT_VBLANK;
        }
        if self.vcount == ((self.dispstat >> 8) & 0xff) {
            value |= DISPSTAT_VCOUNT;
        }
        value
    }

    pub fn vcount(&self) -> u16 {
        self.vcount
    }

    pub fn set_vcount(&mut self, value: u16) {
        self.vcount = value % TOTAL_SCANLINES;
    }

    pub fn write_dispcnt(&mut self, value: u16) {
        self.dispcnt = value;
    }

    pub fn write_dispstat(&mut self, value: u16) {
        self.dispstat = value & (DISPSTAT_IRQ_WRITABLE_MASK | DISPSTAT_VCOUNT_SETTING_MASK);
    }

    pub fn step_scanline(&mut self) {
        self.vcount += 1;
        if self.vcount >= TOTAL_SCANLINES {
            self.vcount = 0;
        }
    }

    pub fn set_hblank(&mut self, active: bool) {
        if active {
            self.dispstat |= DISPSTAT_HBLANK;
        } else {
            self.dispstat &= !DISPSTAT_HBLANK;
        }
    }

    pub fn render_frame(&self, palette: &[u8], vram: &[u8]) -> FrameBuffer {
        match self.dispcnt & DISPCNT_MODE_MASK {
            MODE_3 => self.render_mode3(vram),
            MODE_4 => self.render_mode4(palette, vram),
            _ => vec![0xff000000; WIDTH * HEIGHT],
        }
    }

    pub fn render_mode3(&self, vram: &[u8]) -> FrameBuffer {
        let mut frame = vec![0xff000000; WIDTH * HEIGHT];

        if (self.dispcnt & DISPCNT_MODE_MASK) != MODE_3 || (self.dispcnt & BG2_ENABLE) == 0 {
            return frame;
        }

        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let offset = (y * WIDTH + x) * 2;
                let color = u16::from_le_bytes([vram[offset], vram[offset + 1]]);
                frame[y * WIDTH + x] = bgr555_to_argb8888(color);
            }
        }

        frame
    }

    pub fn render_mode4(&self, palette: &[u8], vram: &[u8]) -> FrameBuffer {
        let mut frame = vec![0xff000000; WIDTH * HEIGHT];

        if (self.dispcnt & DISPCNT_MODE_MASK) != MODE_4 || (self.dispcnt & BG2_ENABLE) == 0 {
            return frame;
        }

        let page_offset = if self.dispcnt & BACKBUFFER != 0 {
            0xA000
        } else {
            0
        };

        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let color_index = usize::from(vram[page_offset + y * WIDTH + x]);
                let palette_offset = color_index * 2;
                let color =
                    u16::from_le_bytes([palette[palette_offset], palette[palette_offset + 1]]);
                frame[y * WIDTH + x] = bgr555_to_argb8888(color);
            }
        }

        frame
    }
}

pub fn bgr555_to_argb8888(color: u16) -> u32 {
    let r5 = u32::from(color & 0x001f);
    let g5 = u32::from((color >> 5) & 0x001f);
    let b5 = u32::from((color >> 10) & 0x001f);

    let r8 = (r5 << 3) | (r5 >> 2);
    let g8 = (g5 << 3) | (g5 >> 2);
    let b8 = (b5 << 3) | (b5 >> 2);

    0xff00_0000 | (r8 << 16) | (g8 << 8) | b8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_bgr555_to_argb8888() {
        assert_eq!(bgr555_to_argb8888(0x001f), 0xffff0000);
        assert_eq!(bgr555_to_argb8888(0x03e0), 0xff00ff00);
        assert_eq!(bgr555_to_argb8888(0x7c00), 0xff0000ff);
        assert_eq!(bgr555_to_argb8888(0x7fff), 0xffffffff);
    }

    #[test]
    fn mode3_uses_vram_when_bg2_is_enabled() {
        let mut vram = vec![0; WIDTH * HEIGHT * 2];
        vram[0] = 0x1f;

        let mut ppu = Ppu::new();
        ppu.write_dispcnt(MODE_3 | BG2_ENABLE);

        let frame = ppu.render_mode3(&vram);

        assert_eq!(frame[0], 0xffff0000);
        assert_eq!(frame[1], 0xff000000);
    }

    #[test]
    fn mode4_uses_palette_and_selected_frame() {
        let mut palette = vec![0; 0x400];
        palette[2..4].copy_from_slice(&rgb5(31, 0, 0).to_le_bytes());
        palette[4..6].copy_from_slice(&rgb5(0, 31, 0).to_le_bytes());

        let mut vram = vec![0; 0x18000];
        vram[0] = 1;
        vram[0xA000] = 2;

        let mut ppu = Ppu::new();
        ppu.write_dispcnt(MODE_4 | BG2_ENABLE);
        assert_eq!(ppu.render_frame(&palette, &vram)[0], 0xffff0000);

        ppu.write_dispcnt(MODE_4 | BG2_ENABLE | BACKBUFFER);
        assert_eq!(ppu.render_frame(&palette, &vram)[0], 0xff00ff00);
    }

    #[test]
    fn mode3_renders_the_initial_sample_pattern() {
        let mut vram = vec![0; WIDTH * HEIGHT * 2];
        for i in 0..20 {
            write_vram_halfword(&mut vram, 5 + i, 5 + i, rgb5(31, 31, 31));
        }
        for i in 0..32 {
            write_vram_halfword(&mut vram, 20 + i * 2, 50, rgb5(i as u16, 0, 0));
            write_vram_halfword(&mut vram, 20 + i * 2, 60, rgb5(0, i as u16, 0));
            write_vram_halfword(&mut vram, 20 + i * 2, 70, rgb5(0, 0, i as u16));
            write_vram_halfword(
                &mut vram,
                20 + i * 2,
                80,
                rgb5(i as u16, i as u16, i as u16),
            );
        }

        let mut ppu = Ppu::new();
        ppu.write_dispcnt(MODE_3 | BG2_ENABLE);
        let frame = ppu.render_mode3(&vram);

        assert_eq!(frame[5 * WIDTH + 5], 0xffffffff);
        assert_eq!(frame[24 * WIDTH + 24], 0xffffffff);
        assert_eq!(frame[50 * WIDTH + 20], 0xff000000);
        assert_eq!(frame[50 * WIDTH + 82], 0xffff0000);
        assert_eq!(frame[60 * WIDTH + 82], 0xff00ff00);
        assert_eq!(frame[70 * WIDTH + 82], 0xff0000ff);
        assert_eq!(frame[80 * WIDTH + 82], 0xffffffff);
    }

    #[test]
    fn vcount_enters_and_leaves_vblank() {
        let mut ppu = Ppu::new();

        for _ in 0..VISIBLE_SCANLINES {
            ppu.step_scanline();
        }
        assert_eq!(ppu.vcount(), VISIBLE_SCANLINES);
        assert_ne!(ppu.dispstat() & DISPSTAT_VBLANK, 0);

        for _ in VISIBLE_SCANLINES..TOTAL_SCANLINES {
            ppu.step_scanline();
        }
        assert_eq!(ppu.vcount(), 0);
        assert_eq!(ppu.dispstat() & DISPSTAT_VBLANK, 0);
    }

    fn write_vram_halfword(vram: &mut [u8], x: usize, y: usize, value: u16) {
        let offset = (y * WIDTH + x) * 2;
        let bytes = value.to_le_bytes();
        vram[offset] = bytes[0];
        vram[offset + 1] = bytes[1];
    }

    fn rgb5(r: u16, g: u16, b: u16) -> u16 {
        (r & 0x1f) | ((g & 0x1f) << 5) | ((b & 0x1f) << 10)
    }
}

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

const BG_ENABLE: [u16; 4] = [BG0_ENABLE, BG1_ENABLE, BG2_ENABLE, BG3_ENABLE];
const WIN_LAYER_BG0: u16 = 1 << 0;
const WIN_LAYER_BG1: u16 = 1 << 1;
const WIN_LAYER_BG2: u16 = 1 << 2;
const WIN_LAYER_BG3: u16 = 1 << 3;
const WIN_LAYER_OBJ: u16 = 1 << 4;
const WIN_LAYER_ALL: u16 =
    WIN_LAYER_BG0 | WIN_LAYER_BG1 | WIN_LAYER_BG2 | WIN_LAYER_BG3 | WIN_LAYER_OBJ;
const WIN_LAYER_BG: [u16; 4] = [WIN_LAYER_BG0, WIN_LAYER_BG1, WIN_LAYER_BG2, WIN_LAYER_BG3];
const BG_MOSAIC: u16 = 1 << 6;
const BG_TILE_SIZE: usize = 8;
const SCREEN_BLOCK_SIZE: usize = 0x800;
const CHAR_BLOCK_SIZE: usize = 0x4000;
const MODE5_WIDTH: usize = 160;
const MODE5_HEIGHT: usize = 128;
const OBJ_TILE_BASE_TEXT_MODE: usize = 0x10000;
const OBJ_TILE_BASE_BITMAP_MODE: usize = 0x14000;
const OBJ_PALETTE_BASE: usize = 0x200;

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
    bgcnt: [u16; 4],
    bghofs: [u16; 4],
    bgvofs: [u16; 4],
    winh: [u16; 2],
    winv: [u16; 2],
    winin: u16,
    winout: u16,
    mosaic: u16,
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

    pub fn bgcnt(&self, bg: usize) -> u16 {
        self.bgcnt[bg]
    }

    pub fn bghofs(&self, bg: usize) -> u16 {
        self.bghofs[bg]
    }

    pub fn bgvofs(&self, bg: usize) -> u16 {
        self.bgvofs[bg]
    }

    pub fn mosaic(&self) -> u16 {
        self.mosaic
    }

    pub fn winh(&self, window: usize) -> u16 {
        self.winh[window]
    }

    pub fn winv(&self, window: usize) -> u16 {
        self.winv[window]
    }

    pub fn winin(&self) -> u16 {
        self.winin
    }

    pub fn winout(&self) -> u16 {
        self.winout
    }

    pub fn write_bgcnt(&mut self, bg: usize, value: u16) {
        self.bgcnt[bg] = value;
    }

    pub fn write_bghofs(&mut self, bg: usize, value: u16) {
        self.bghofs[bg] = value & 0x01ff;
    }

    pub fn write_bgvofs(&mut self, bg: usize, value: u16) {
        self.bgvofs[bg] = value & 0x01ff;
    }

    pub fn write_winh(&mut self, window: usize, value: u16) {
        self.winh[window] = value;
    }

    pub fn write_winv(&mut self, window: usize, value: u16) {
        self.winv[window] = value;
    }

    pub fn write_winin(&mut self, value: u16) {
        self.winin = value & 0x3f3f;
    }

    pub fn write_winout(&mut self, value: u16) {
        self.winout = value & 0x3f3f;
    }

    pub fn write_mosaic(&mut self, value: u16) {
        self.mosaic = value;
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

    pub fn render_frame(&self, palette: &[u8], vram: &[u8], oam: &[u8]) -> FrameBuffer {
        let mut frame = match self.dispcnt & DISPCNT_MODE_MASK {
            MODE_0 => self.render_mode0(palette, vram),
            MODE_3 => self.render_mode3(vram),
            MODE_4 => self.render_mode4(palette, vram),
            MODE_5 => self.render_mode5(vram),
            _ => vec![0xff000000; WIDTH * HEIGHT],
        };

        if self.dispcnt & OBJ_ENABLE != 0 {
            self.render_objs(palette, vram, oam, &mut frame);
        }

        frame
    }

    pub fn render_mode0(&self, palette: &[u8], vram: &[u8]) -> FrameBuffer {
        let backdrop = read_u16_checked(palette, 0).map_or(0xff000000, bgr555_to_argb8888);
        let mut frame = vec![backdrop; WIDTH * HEIGHT];

        if (self.dispcnt & DISPCNT_MODE_MASK) != MODE_0 {
            return frame;
        }

        let mut bgs = [0usize, 1, 2, 3];
        bgs.sort_by_key(|&bg| (self.bg_priority(bg), bg));

        if !self.windows_enabled() {
            for bg in bgs.into_iter().rev() {
                if self.dispcnt & BG_ENABLE[bg] == 0 {
                    continue;
                }
                self.render_text_bg(bg, palette, vram, &mut frame);
            }
            return frame;
        }

        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let mask = self.window_mask(x, y);
                for bg in bgs {
                    if self.dispcnt & BG_ENABLE[bg] == 0 || mask & WIN_LAYER_BG[bg] == 0 {
                        continue;
                    }
                    if let Some(color) = self.text_bg_pixel(bg, x, y, palette, vram) {
                        frame[y * WIDTH + x] = color;
                        break;
                    }
                }
            }
        }

        frame
    }

    pub fn render_mode3(&self, vram: &[u8]) -> FrameBuffer {
        let mut frame = vec![0xff000000; WIDTH * HEIGHT];

        if (self.dispcnt & DISPCNT_MODE_MASK) != MODE_3 || (self.dispcnt & BG2_ENABLE) == 0 {
            return frame;
        }

        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let (source_x, source_y) = self.bg2_bitmap_source_pixel(x, y);
                let offset = (source_y * WIDTH + source_x) * 2;
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
                let (source_x, source_y) = self.bg2_bitmap_source_pixel(x, y);
                let color_index = usize::from(vram[page_offset + source_y * WIDTH + source_x]);
                let palette_offset = color_index * 2;
                let color =
                    u16::from_le_bytes([palette[palette_offset], palette[palette_offset + 1]]);
                frame[y * WIDTH + x] = bgr555_to_argb8888(color);
            }
        }

        frame
    }

    pub fn render_mode5(&self, vram: &[u8]) -> FrameBuffer {
        let mut frame = vec![0xff000000; WIDTH * HEIGHT];

        if (self.dispcnt & DISPCNT_MODE_MASK) != MODE_5 || (self.dispcnt & BG2_ENABLE) == 0 {
            return frame;
        }

        let page_offset = if self.dispcnt & BACKBUFFER != 0 {
            0xA000
        } else {
            0
        };

        for y in 0..MODE5_HEIGHT {
            for x in 0..MODE5_WIDTH {
                let (source_x, source_y) = self.bg2_bitmap_source_pixel(x, y);
                let source_x = source_x.min(MODE5_WIDTH - 1);
                let source_y = source_y.min(MODE5_HEIGHT - 1);
                let offset = page_offset + (source_y * MODE5_WIDTH + source_x) * 2;
                let color = u16::from_le_bytes([vram[offset], vram[offset + 1]]);
                frame[y * WIDTH + x] = bgr555_to_argb8888(color);
            }
        }

        frame
    }

    fn render_objs(&self, palette: &[u8], vram: &[u8], oam: &[u8], frame: &mut [u32]) {
        let mut objs = Vec::with_capacity(128);
        for obj_index in 0..128 {
            let offset = obj_index * 8;
            let attr0 = read_u16(oam, offset);
            let attr1 = read_u16(oam, offset + 2);
            let attr2 = read_u16(oam, offset + 4);

            if attr0 & (1 << 9) != 0 {
                continue;
            }
            if ((attr0 >> 10) & 0x3) != 0 {
                continue;
            }
            if attr0 & (1 << 13) != 0 {
                continue;
            }

            objs.push(Obj {
                index: obj_index,
                attr0,
                attr1,
                attr2,
            });
        }

        objs.sort_by_key(|obj| {
            (
                std::cmp::Reverse(obj.priority()),
                std::cmp::Reverse(obj.index),
            )
        });

        for obj in objs {
            let attr0 = obj.attr0;
            let attr1 = obj.attr1;
            let attr2 = obj.attr2;
            let (obj_width, obj_height) = obj_size(attr0, attr1);
            let obj_x = sign_extend(attr1 & 0x01ff, 9);
            let obj_y = sign_extend(attr0 & 0x00ff, 8);
            let tile_base = usize::from(attr2 & 0x03ff);
            let palette_bank = usize::from((attr2 >> 12) & 0x0f);
            let tile_memory_base = self.obj_tile_memory_base();
            let one_dimensional = self.dispcnt & OBJ_1D_MAPPING != 0;
            let row_stride = if one_dimensional { obj_width / 8 } else { 32 };

            for y in 0..obj_height {
                let screen_y = obj_y + y as i32;
                if !(0..HEIGHT as i32).contains(&screen_y) {
                    continue;
                }

                for x in 0..obj_width {
                    let screen_x = obj_x + x as i32;
                    if !(0..WIDTH as i32).contains(&screen_x) {
                        continue;
                    }

                    let tile_x = x / 8;
                    let tile_y = y / 8;
                    let tile_number = tile_base + tile_y * row_stride + tile_x;
                    let pixel_in_tile = (y % 8) * 8 + (x % 8);
                    let color_index =
                        obj_4bpp_color(vram, tile_memory_base, tile_number, pixel_in_tile);
                    if color_index == 0 {
                        continue;
                    }

                    let palette_index =
                        OBJ_PALETTE_BASE + (palette_bank * 16 + usize::from(color_index)) * 2;
                    let color =
                        u16::from_le_bytes([palette[palette_index], palette[palette_index + 1]]);
                    frame[screen_y as usize * WIDTH + screen_x as usize] =
                        bgr555_to_argb8888(color);
                }
            }
        }
    }

    fn obj_tile_memory_base(&self) -> usize {
        match self.dispcnt & DISPCNT_MODE_MASK {
            MODE_0..=0x0002 => OBJ_TILE_BASE_TEXT_MODE,
            _ => OBJ_TILE_BASE_BITMAP_MODE,
        }
    }

    fn bg2_bitmap_source_pixel(&self, x: usize, y: usize) -> (usize, usize) {
        if self.bgcnt[2] & BG_MOSAIC == 0 {
            return (x, y);
        }

        let (mosaic_width, mosaic_height) = bg_mosaic_size(self.mosaic);
        (
            x / mosaic_width * mosaic_width,
            y / mosaic_height * mosaic_height,
        )
    }

    fn render_text_bg(&self, bg: usize, palette: &[u8], vram: &[u8], frame: &mut [u32]) {
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                if let Some(color) = self.text_bg_pixel(bg, x, y, palette, vram) {
                    frame[y * WIDTH + x] = color;
                }
            }
        }
    }

    fn text_bg_pixel(
        &self,
        bg: usize,
        screen_x: usize,
        screen_y: usize,
        palette: &[u8],
        vram: &[u8],
    ) -> Option<u32> {
        let bgcnt = self.bgcnt[bg];
        let char_base = usize::from((bgcnt >> 2) & 0x3) * CHAR_BLOCK_SIZE;
        let screen_base = usize::from((bgcnt >> 8) & 0x1f) * SCREEN_BLOCK_SIZE;
        let is_8bpp = bgcnt & (1 << 7) != 0;
        let (bg_width, bg_height) = text_bg_size(bgcnt);

        let bg_x = (screen_x + usize::from(self.bghofs[bg])) % bg_width;
        let bg_y = (screen_y + usize::from(self.bgvofs[bg])) % bg_height;
        let color_index =
            text_bg_color_index(vram, char_base, screen_base, is_8bpp, bg_width, bg_x, bg_y)?;
        if color_index == 0 {
            return None;
        }
        read_u16_checked(palette, usize::from(color_index) * 2).map(bgr555_to_argb8888)
    }

    fn bg_priority(&self, bg: usize) -> u16 {
        self.bgcnt[bg] & 0x3
    }

    fn windows_enabled(&self) -> bool {
        self.dispcnt & (WIN0_ENABLE | WIN1_ENABLE | OBJ_WIN_ENABLE) != 0
    }

    fn window_mask(&self, x: usize, y: usize) -> u16 {
        if self.dispcnt & WIN0_ENABLE != 0 && self.window_contains(0, x, y) {
            self.winin & 0x003f
        } else if self.dispcnt & WIN1_ENABLE != 0 && self.window_contains(1, x, y) {
            (self.winin >> 8) & 0x003f
        } else {
            self.winout & WIN_LAYER_ALL
        }
    }

    fn window_contains(&self, window: usize, x: usize, y: usize) -> bool {
        let left = usize::from((self.winh[window] >> 8) & 0xff).min(WIDTH);
        let right = usize::from(self.winh[window] & 0xff).min(WIDTH);
        let top = usize::from((self.winv[window] >> 8) & 0xff).min(HEIGHT);
        let bottom = usize::from(self.winv[window] & 0xff).min(HEIGHT);
        x >= left && x < right && y >= top && y < bottom
    }
}

#[derive(Clone, Copy)]
struct Obj {
    index: usize,
    attr0: u16,
    attr1: u16,
    attr2: u16,
}

impl Obj {
    fn priority(self) -> u16 {
        (self.attr2 >> 10) & 0x3
    }
}

fn obj_4bpp_color(vram: &[u8], base: usize, tile_number: usize, pixel_in_tile: usize) -> u8 {
    let tile_offset = base + tile_number * 32;
    if tile_offset + 31 >= vram.len() {
        return 0;
    }
    let color_byte = vram[tile_offset + pixel_in_tile / 2];
    if pixel_in_tile & 1 == 0 {
        color_byte & 0x0f
    } else {
        color_byte >> 4
    }
}

fn text_bg_color_index(
    vram: &[u8],
    char_base: usize,
    screen_base: usize,
    is_8bpp: bool,
    bg_width: usize,
    bg_x: usize,
    bg_y: usize,
) -> Option<u16> {
    let tile_x = bg_x / BG_TILE_SIZE;
    let tile_y = bg_y / BG_TILE_SIZE;
    let screen_block = match (bg_width > 256, bg_y >= 256) {
        (false, false) => 0,
        (true, false) => tile_x / 32,
        (false, true) => 1,
        (true, true) => 2 + tile_x / 32,
    };
    let map_x = tile_x % 32;
    let map_y = tile_y % 32;
    let map_offset = screen_base + screen_block * SCREEN_BLOCK_SIZE + (map_y * 32 + map_x) * 2;
    let entry = read_u16_checked(vram, map_offset)?;

    let tile_number = usize::from(entry & 0x03ff);
    let mut pixel_x = bg_x % BG_TILE_SIZE;
    let mut pixel_y = bg_y % BG_TILE_SIZE;
    if entry & (1 << 10) != 0 {
        pixel_x = BG_TILE_SIZE - 1 - pixel_x;
    }
    if entry & (1 << 11) != 0 {
        pixel_y = BG_TILE_SIZE - 1 - pixel_y;
    }

    if is_8bpp {
        let tile_offset = char_base + tile_number * 64 + pixel_y * BG_TILE_SIZE + pixel_x;
        vram.get(tile_offset).copied().map(u16::from)
    } else {
        let tile_offset = char_base + tile_number * 32 + (pixel_y * BG_TILE_SIZE + pixel_x) / 2;
        let byte = *vram.get(tile_offset)?;
        let color = if pixel_x & 1 == 0 {
            byte & 0x0f
        } else {
            byte >> 4
        };
        Some(((entry >> 12) & 0x0f) * 16 + u16::from(color))
    }
}

fn text_bg_size(bgcnt: u16) -> (usize, usize) {
    match (bgcnt >> 14) & 0x3 {
        0 => (256, 256),
        1 => (512, 256),
        2 => (256, 512),
        _ => (512, 512),
    }
}

fn bg_mosaic_size(mosaic: u16) -> (usize, usize) {
    (
        usize::from(mosaic & 0x000f) + 1,
        usize::from((mosaic >> 4) & 0x000f) + 1,
    )
}

fn read_u16_checked(memory: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        *memory.get(offset)?,
        *memory.get(offset + 1)?,
    ]))
}

fn read_u16(memory: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([memory[offset], memory[offset + 1]])
}

fn sign_extend(value: u16, bits: u32) -> i32 {
    let sign_bit = 1u16 << (bits - 1);
    if value & sign_bit != 0 {
        i32::from(value) - (1 << bits)
    } else {
        i32::from(value)
    }
}

fn obj_size(attr0: u16, attr1: u16) -> (usize, usize) {
    match ((attr0 >> 14) & 0x3, (attr1 >> 14) & 0x3) {
        (0, 0) => (8, 8),
        (0, 1) => (16, 16),
        (0, 2) => (32, 32),
        (0, 3) => (64, 64),
        (1, 0) => (16, 8),
        (1, 1) => (32, 8),
        (1, 2) => (32, 16),
        (1, 3) => (64, 32),
        (2, 0) => (8, 16),
        (2, 1) => (8, 32),
        (2, 2) => (16, 32),
        (2, 3) => (32, 64),
        _ => (8, 8),
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
    fn mode3_applies_bg2_mosaic() {
        let mut vram = vec![0; WIDTH * HEIGHT * 2];
        vram[0..2].copy_from_slice(&rgb5(31, 0, 0).to_le_bytes());
        vram[2..4].copy_from_slice(&rgb5(0, 31, 0).to_le_bytes());
        vram[WIDTH * 2..WIDTH * 2 + 2].copy_from_slice(&rgb5(0, 0, 31).to_le_bytes());

        let mut ppu = Ppu::new();
        ppu.write_dispcnt(MODE_3 | BG2_ENABLE);
        ppu.write_bgcnt(2, BG_MOSAIC);
        ppu.write_mosaic(0x0011);

        let frame = ppu.render_mode3(&vram);

        assert_eq!(frame[0], 0xffff0000);
        assert_eq!(frame[1], 0xffff0000);
        assert_eq!(frame[WIDTH], 0xffff0000);
        assert_eq!(frame[WIDTH + 2], 0xff000000);
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
        let oam = vec![0; 0x400];
        assert_eq!(ppu.render_frame(&palette, &vram, &oam)[0], 0xffff0000);

        ppu.write_dispcnt(MODE_4 | BG2_ENABLE | BACKBUFFER);
        assert_eq!(ppu.render_frame(&palette, &vram, &oam)[0], 0xff00ff00);
    }

    #[test]
    fn mode5_uses_16bpp_pixels_and_selected_frame() {
        let palette = vec![0; 0x400];
        let mut vram = vec![0; 0x18000];
        vram[0..2].copy_from_slice(&rgb5(31, 0, 0).to_le_bytes());
        vram[0xA000..0xA002].copy_from_slice(&rgb5(0, 0, 31).to_le_bytes());

        let mut ppu = Ppu::new();
        ppu.write_dispcnt(MODE_5 | BG2_ENABLE);
        let oam = vec![0; 0x400];
        assert_eq!(ppu.render_frame(&palette, &vram, &oam)[0], 0xffff0000);

        ppu.write_dispcnt(MODE_5 | BG2_ENABLE | BACKBUFFER);
        assert_eq!(ppu.render_frame(&palette, &vram, &oam)[0], 0xff0000ff);
    }

    #[test]
    fn mode0_renders_bg0_8bpp_text_tiles() {
        let mut palette = vec![0; 0x400];
        palette[2..4].copy_from_slice(&rgb5(31, 0, 0).to_le_bytes());

        let mut vram = vec![0; 0x18000];
        vram[64] = 1;
        write_vram_halfword_offset(&mut vram, 8 * SCREEN_BLOCK_SIZE, 1);

        let mut ppu = Ppu::new();
        ppu.write_dispcnt(MODE_0 | BG0_ENABLE);
        ppu.write_bgcnt(0, (1 << 7) | (8 << 8));

        let frame = ppu.render_mode0(&palette, &vram);

        assert_eq!(frame[0], 0xffff0000);
        assert_eq!(frame[1], 0xff000000);
    }

    #[test]
    fn mode0_composes_bg0_over_bg1_with_4bpp_palette_banks() {
        let mut palette = vec![0; 0x400];
        palette[2..4].copy_from_slice(&rgb5(31, 0, 0).to_le_bytes());
        palette[16 * 2 + 2..16 * 2 + 4].copy_from_slice(&rgb5(0, 31, 0).to_le_bytes());

        let mut vram = vec![0; 0x18000];
        vram[0] = 0x10;
        vram[2 * CHAR_BLOCK_SIZE] = 0x11;
        write_vram_halfword_offset(&mut vram, 11 * SCREEN_BLOCK_SIZE, 0);
        write_vram_halfword_offset(&mut vram, 12 * SCREEN_BLOCK_SIZE, 1 << 12);

        let mut ppu = Ppu::new();
        ppu.write_dispcnt(MODE_0 | BG0_ENABLE | BG1_ENABLE);
        ppu.write_bgcnt(0, 11 << 8);
        ppu.write_bgcnt(1, (2 << 2) | (12 << 8));

        let frame = ppu.render_mode0(&palette, &vram);

        assert_eq!(frame[0], 0xff00ff00);
        assert_eq!(frame[1], 0xffff0000);
    }

    #[test]
    fn mode0_win0_selects_different_backgrounds_inside_and_outside() {
        let mut palette = vec![0; 0x400];
        palette[2..4].copy_from_slice(&rgb5(31, 0, 0).to_le_bytes());
        palette[16 * 2 + 2..16 * 2 + 4].copy_from_slice(&rgb5(0, 31, 0).to_le_bytes());

        let mut vram = vec![0; 0x18000];
        vram[0..32].fill(0x11);
        vram[2 * CHAR_BLOCK_SIZE..2 * CHAR_BLOCK_SIZE + 32].fill(0x11);
        for tile in 0..32 * 32 {
            write_vram_halfword_offset(&mut vram, 11 * SCREEN_BLOCK_SIZE + tile * 2, 0);
            write_vram_halfword_offset(&mut vram, 12 * SCREEN_BLOCK_SIZE + tile * 2, 1 << 12);
        }

        let mut ppu = Ppu::new();
        ppu.write_dispcnt(MODE_0 | BG0_ENABLE | BG1_ENABLE | WIN0_ENABLE);
        ppu.write_bgcnt(0, 11 << 8);
        ppu.write_bgcnt(1, (2 << 2) | (12 << 8));
        ppu.write_winh(0, (20 << 8) | 84);
        ppu.write_winv(0, (20 << 8) | 84);
        ppu.write_winin(WIN_LAYER_BG1);
        ppu.write_winout(WIN_LAYER_BG0);

        let frame = ppu.render_mode0(&palette, &vram);

        assert_eq!(frame[19 * WIDTH + 20], 0xffff0000);
        assert_eq!(frame[20 * WIDTH + 20], 0xff00ff00);
        assert_eq!(frame[83 * WIDTH + 83], 0xff00ff00);
        assert_eq!(frame[84 * WIDTH + 83], 0xffff0000);
    }

    #[test]
    fn mode0_renders_4bpp_square_obj() {
        let mut palette = vec![0; 0x400];
        palette[OBJ_PALETTE_BASE + 2..OBJ_PALETTE_BASE + 4]
            .copy_from_slice(&rgb5(31, 31, 31).to_le_bytes());

        let mut vram = vec![0; 0x18000];
        vram[OBJ_TILE_BASE_TEXT_MODE + 32] = 0x11;

        let mut oam = vec![0; 0x400];
        write_oam_halfword(&mut oam, 0, 0);
        write_oam_halfword(&mut oam, 2, 0);
        write_oam_halfword(&mut oam, 4, 1);

        let mut ppu = Ppu::new();
        ppu.write_dispcnt(MODE_0 | OBJ_ENABLE);
        let frame = ppu.render_frame(&palette, &vram, &oam);

        assert_eq!(frame[0], 0xffffffff);
        assert_eq!(frame[2], 0xff000000);
    }

    #[test]
    fn attr2_palette_bank_selects_obj_palette_row() {
        let mut palette = vec![0; 0x400];
        palette[OBJ_PALETTE_BASE + 2..OBJ_PALETTE_BASE + 4]
            .copy_from_slice(&rgb5(31, 0, 0).to_le_bytes());
        palette[OBJ_PALETTE_BASE + 16 * 2 + 2..OBJ_PALETTE_BASE + 16 * 2 + 4]
            .copy_from_slice(&rgb5(0, 31, 0).to_le_bytes());

        let mut vram = vec![0; 0x18000];
        vram[OBJ_TILE_BASE_TEXT_MODE] = 0x11;

        let mut oam = vec![0; 0x400];
        write_oam_halfword(&mut oam, 0, 0);
        write_oam_halfword(&mut oam, 2, 0);
        write_oam_halfword(&mut oam, 4, 1 << 12);

        let mut ppu = Ppu::new();
        ppu.write_dispcnt(MODE_0 | OBJ_ENABLE | OBJ_1D_MAPPING);
        let frame = ppu.render_frame(&palette, &vram, &oam);

        assert_eq!(frame[0], 0xff00ff00);
    }

    #[test]
    fn attr2_priority_and_oam_index_control_obj_order() {
        let mut palette = vec![0; 0x400];
        palette[OBJ_PALETTE_BASE + 2..OBJ_PALETTE_BASE + 4]
            .copy_from_slice(&rgb5(31, 0, 0).to_le_bytes());
        palette[OBJ_PALETTE_BASE + 4..OBJ_PALETTE_BASE + 6]
            .copy_from_slice(&rgb5(0, 31, 0).to_le_bytes());

        let mut vram = vec![0; 0x18000];
        vram[OBJ_TILE_BASE_TEXT_MODE] = 0x11;
        vram[OBJ_TILE_BASE_TEXT_MODE + 32] = 0x22;

        let mut oam = vec![0; 0x400];
        write_oam_halfword(&mut oam, 0, 0);
        write_oam_halfword(&mut oam, 2, 0);
        write_oam_halfword(&mut oam, 4, 0 | (1 << 10));
        write_oam_halfword(&mut oam, 8, 0);
        write_oam_halfword(&mut oam, 10, 0);
        write_oam_halfword(&mut oam, 12, 1);

        let mut ppu = Ppu::new();
        ppu.write_dispcnt(MODE_0 | OBJ_ENABLE | OBJ_1D_MAPPING);
        assert_eq!(ppu.render_frame(&palette, &vram, &oam)[0], 0xff00ff00);

        write_oam_halfword(&mut oam, 4, 0);
        write_oam_halfword(&mut oam, 12, 1);
        assert_eq!(ppu.render_frame(&palette, &vram, &oam)[0], 0xffff0000);
    }

    #[test]
    fn obj_1d_mapping_uses_compact_rows() {
        let mut palette = vec![0; 0x400];
        palette[OBJ_PALETTE_BASE + 2..OBJ_PALETTE_BASE + 4]
            .copy_from_slice(&rgb5(31, 31, 31).to_le_bytes());

        let mut vram = vec![0; 0x18000];
        vram[OBJ_TILE_BASE_TEXT_MODE + 9 * 32] = 0x11;

        let mut oam = vec![0; 0x400];
        write_oam_halfword(&mut oam, 0, 0);
        write_oam_halfword(&mut oam, 2, 3 << 14);
        write_oam_halfword(&mut oam, 4, 1);

        let mut ppu = Ppu::new();
        ppu.write_dispcnt(MODE_0 | OBJ_ENABLE | OBJ_1D_MAPPING);
        let frame = ppu.render_frame(&palette, &vram, &oam);

        assert_eq!(frame[WIDTH * 8], 0xffffffff);
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
        write_vram_halfword_offset(vram, offset, value);
    }

    fn write_vram_halfword_offset(vram: &mut [u8], offset: usize, value: u16) {
        let bytes = value.to_le_bytes();
        vram[offset] = bytes[0];
        vram[offset + 1] = bytes[1];
    }

    fn rgb5(r: u16, g: u16, b: u16) -> u16 {
        (r & 0x1f) | ((g & 0x1f) << 5) | ((b & 0x1f) << 10)
    }

    fn write_oam_halfword(oam: &mut [u8], offset: usize, value: u16) {
        let bytes = value.to_le_bytes();
        oam[offset] = bytes[0];
        oam[offset + 1] = bytes[1];
    }
}

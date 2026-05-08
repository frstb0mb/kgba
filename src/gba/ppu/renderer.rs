use super::{
    BACKBUFFER, BG0_ENABLE, BG1_ENABLE, BG2_ENABLE, BG3_ENABLE, DISPCNT_MODE_MASK, HEIGHT, MODE_0,
    MODE_3, MODE_4, MODE_5, OBJ_1D_MAPPING, OBJ_ENABLE, OBJ_WIN_ENABLE, Ppu, WIDTH, WIN0_ENABLE,
    WIN1_ENABLE,
};

pub type FrameBuffer = Vec<u32>;

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

impl Ppu {
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
mod tests;

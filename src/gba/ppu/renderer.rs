use super::{
    BACKBUFFER, BG0_ENABLE, BG1_ENABLE, BG2_ENABLE, BG3_ENABLE, DISPCNT_MODE_MASK, HEIGHT, MODE_0,
    MODE_3, MODE_4, MODE_5, OBJ_1D_MAPPING, OBJ_ENABLE, OBJ_WIN_ENABLE, Ppu, WIDTH, WIN0_ENABLE,
    WIN1_ENABLE,
};

pub type FrameBuffer = Vec<u16>;

const BG_ENABLE: [u16; 4] = [BG0_ENABLE, BG1_ENABLE, BG2_ENABLE, BG3_ENABLE];
const WIN_LAYER_BG0: u16 = 1 << 0;
const WIN_LAYER_BG1: u16 = 1 << 1;
const WIN_LAYER_BG2: u16 = 1 << 2;
const WIN_LAYER_BG3: u16 = 1 << 3;
const WIN_LAYER_OBJ: u16 = 1 << 4;
const WIN_LAYER_EFFECT: u16 = 1 << 5;
const WIN_LAYER_ALL: u16 =
    WIN_LAYER_BG0 | WIN_LAYER_BG1 | WIN_LAYER_BG2 | WIN_LAYER_BG3 | WIN_LAYER_OBJ;
const WIN_MASK_ALL: u16 = WIN_LAYER_ALL | WIN_LAYER_EFFECT;
const WIN_LAYER_BG: [u16; 4] = [WIN_LAYER_BG0, WIN_LAYER_BG1, WIN_LAYER_BG2, WIN_LAYER_BG3];
const BLEND_LAYER_BG: [u16; 4] = [1 << 0, 1 << 1, 1 << 2, 1 << 3];
const BLEND_LAYER_BACKDROP: u16 = 1 << 5;
const BLEND_MODE_ALPHA: u16 = 1;
const BLEND_MODE_LIGHTEN: u16 = 2;
const BLEND_MODE_DARKEN: u16 = 3;
const BG_MOSAIC: u16 = 1 << 6;
const BG_TILE_SIZE: usize = 8;
const SCREEN_BLOCK_SIZE: usize = 0x800;
const CHAR_BLOCK_SIZE: usize = 0x4000;
const MODE5_WIDTH: usize = 160;
const MODE5_HEIGHT: usize = 128;
const OBJ_TILE_BASE: usize = 0x10000;
const OBJ_PALETTE_BASE: usize = 0x200;

impl Ppu {
    pub fn render_frame(&self, palette: &[u8], vram: &[u8], oam: &[u8]) -> FrameBuffer {
        let mut frame = match self.dispcnt & DISPCNT_MODE_MASK {
            MODE_0 => self.render_mode0(palette, vram),
            MODE_3 => self.render_mode3(vram),
            MODE_4 => self.render_mode4(palette, vram),
            MODE_5 => self.render_mode5(vram),
            _ => vec![0; WIDTH * HEIGHT],
        };

        if self.dispcnt & OBJ_ENABLE != 0 {
            self.render_objs(palette, vram, oam, &mut frame);
        }

        frame
    }

    pub fn render_mode0(&self, palette: &[u8], vram: &[u8]) -> FrameBuffer {
        let backdrop = read_u16_checked(palette, 0).unwrap_or(0);
        let mut frame = vec![backdrop; WIDTH * HEIGHT];

        if (self.dispcnt & DISPCNT_MODE_MASK) != MODE_0 {
            return frame;
        }

        let bg_order = self.sorted_bg_order();
        let effects_enabled_everywhere = !self.windows_enabled();

        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let mask = if self.windows_enabled() {
                    self.window_mask(x, y)
                } else {
                    WIN_MASK_ALL
                };
                let mut layers = Vec::with_capacity(5);
                for bg in bg_order {
                    if self.dispcnt & BG_ENABLE[bg] == 0 || mask & WIN_LAYER_BG[bg] == 0 {
                        continue;
                    }
                    if let Some(color) = self.text_bg_pixel_raw(bg, x, y, palette, vram) {
                        layers.push(RenderedLayer {
                            color,
                            blend_layer: BLEND_LAYER_BG[bg],
                        });
                    }
                }
                layers.push(RenderedLayer {
                    color: backdrop,
                    blend_layer: BLEND_LAYER_BACKDROP,
                });

                let effects_enabled = effects_enabled_everywhere || mask & WIN_LAYER_EFFECT != 0;
                let color = if effects_enabled {
                    self.apply_blend(layers[0], layers.get(1).copied())
                } else {
                    layers[0].color
                };
                frame[y * WIDTH + x] = color;
            }
        }

        frame
    }

    pub fn render_mode0_scanline(&self, y: usize, palette: &[u8], vram: &[u8], line: &mut [u16]) {
        if y >= HEIGHT || line.len() < WIDTH {
            return;
        }

        let backdrop = read_u16_checked(palette, 0).unwrap_or(0);
        line[..WIDTH].fill(backdrop);

        if (self.dispcnt & DISPCNT_MODE_MASK) != MODE_0 {
            return;
        }

        let bg_order = self.sorted_bg_order();
        for (x, pixel) in line.iter_mut().take(WIDTH).enumerate() {
            for bg in bg_order {
                if self.dispcnt & BG_ENABLE[bg] == 0 {
                    continue;
                }
                if let Some(color) = self.text_bg_pixel_raw(bg, x, y, palette, vram) {
                    *pixel = color;
                    break;
                }
            }
        }
    }

    pub fn render_mode3(&self, vram: &[u8]) -> FrameBuffer {
        let mut frame = vec![0; WIDTH * HEIGHT];

        if (self.dispcnt & DISPCNT_MODE_MASK) != MODE_3 || (self.dispcnt & BG2_ENABLE) == 0 {
            return frame;
        }

        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let Some((source_x, source_y)) = self.bg2_bitmap_source_pixel(x, y, WIDTH, HEIGHT)
                else {
                    continue;
                };
                let offset = (source_y * WIDTH + source_x) * 2;
                let color = u16::from_le_bytes([vram[offset], vram[offset + 1]]);
                frame[y * WIDTH + x] = color;
            }
        }

        frame
    }

    pub fn render_mode4(&self, palette: &[u8], vram: &[u8]) -> FrameBuffer {
        let mut frame = vec![0; WIDTH * HEIGHT];

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
                let Some((source_x, source_y)) = self.bg2_bitmap_source_pixel(x, y, WIDTH, HEIGHT)
                else {
                    continue;
                };
                let color_index = usize::from(vram[page_offset + source_y * WIDTH + source_x]);
                let palette_offset = color_index * 2;
                let color =
                    u16::from_le_bytes([palette[palette_offset], palette[palette_offset + 1]]);
                frame[y * WIDTH + x] = color;
            }
        }

        frame
    }

    pub fn render_mode5(&self, vram: &[u8]) -> FrameBuffer {
        let mut frame = vec![0; WIDTH * HEIGHT];

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
                let Some((source_x, source_y)) =
                    self.bg2_bitmap_source_pixel(x, y, MODE5_WIDTH, MODE5_HEIGHT)
                else {
                    continue;
                };
                let offset = page_offset + (source_y * MODE5_WIDTH + source_x) * 2;
                let color = u16::from_le_bytes([vram[offset], vram[offset + 1]]);
                frame[y * WIDTH + x] = color;
            }
        }

        frame
    }

    fn render_objs(&self, palette: &[u8], vram: &[u8], oam: &[u8], frame: &mut [u16]) {
        let mut objs = Vec::with_capacity(128);
        for obj_index in 0..128 {
            let offset = obj_index * 8;
            let attr0 = read_u16(oam, offset);
            let attr1 = read_u16(oam, offset + 2);
            let attr2 = read_u16(oam, offset + 4);

            let affine = attr0 & (1 << 8) != 0;
            if !affine && attr0 & (1 << 9) != 0 {
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
            let affine = attr0 & (1 << 8) != 0;
            let double_size = affine && attr0 & (1 << 9) != 0;
            let (draw_width, draw_height) = if double_size {
                (obj_width * 2, obj_height * 2)
            } else {
                (obj_width, obj_height)
            };
            let obj_x = sign_extend(attr1 & 0x01ff, 9);
            let obj_y = sign_extend(attr0 & 0x00ff, 8);
            let tile_base = usize::from(attr2 & 0x03ff);
            let palette_bank = usize::from((attr2 >> 12) & 0x0f);
            let tile_memory_base = OBJ_TILE_BASE;
            let one_dimensional = self.dispcnt & OBJ_1D_MAPPING != 0;
            let row_stride = if one_dimensional { obj_width / 8 } else { 32 };
            let affine_params =
                affine.then(|| obj_affine_params(oam, usize::from((attr1 >> 9) & 0x1f)));

            for y in 0..draw_height {
                let screen_y = obj_y + y as i32;
                if !(0..HEIGHT as i32).contains(&screen_y) {
                    continue;
                }

                for x in 0..draw_width {
                    let screen_x = obj_x + x as i32;
                    if !(0..WIDTH as i32).contains(&screen_x) {
                        continue;
                    }

                    let Some((source_x, source_y)) = obj_source_pixel(
                        x,
                        y,
                        obj_width,
                        obj_height,
                        draw_width,
                        draw_height,
                        affine_params,
                    ) else {
                        continue;
                    };

                    let tile_x = source_x / 8;
                    let tile_y = source_y / 8;
                    let tile_number = tile_base + tile_y * row_stride + tile_x;
                    let pixel_in_tile = (source_y % 8) * 8 + (source_x % 8);
                    let color_index =
                        obj_4bpp_color(vram, tile_memory_base, tile_number, pixel_in_tile);
                    if color_index == 0 {
                        continue;
                    }

                    let palette_index =
                        OBJ_PALETTE_BASE + (palette_bank * 16 + usize::from(color_index)) * 2;
                    let color =
                        u16::from_le_bytes([palette[palette_index], palette[palette_index + 1]]);
                    frame[screen_y as usize * WIDTH + screen_x as usize] = color;
                }
            }
        }
    }

    fn bg2_bitmap_source_pixel(
        &self,
        x: usize,
        y: usize,
        bitmap_width: usize,
        bitmap_height: usize,
    ) -> Option<(usize, usize)> {
        let (screen_x, screen_y) = if self.bgcnt[2] & BG_MOSAIC == 0 {
            (x, y)
        } else {
            let (mosaic_width, mosaic_height) = bg_mosaic_size(self.mosaic);
            (
                x / mosaic_width * mosaic_width,
                y / mosaic_height * mosaic_height,
            )
        };

        let source_x =
            affine_source_coord(self.bgx[0], self.bgpa[0], self.bgpb[0], screen_x, screen_y);
        let source_y =
            affine_source_coord(self.bgy[0], self.bgpc[0], self.bgpd[0], screen_x, screen_y);

        if source_x < 0
            || source_y < 0
            || source_x >= bitmap_width as i32
            || source_y >= bitmap_height as i32
        {
            return None;
        }
        Some((source_x as usize, source_y as usize))
    }

    fn text_bg_pixel_raw(
        &self,
        bg: usize,
        screen_x: usize,
        screen_y: usize,
        palette: &[u8],
        vram: &[u8],
    ) -> Option<u16> {
        let bgcnt = self.bgcnt[bg];
        let char_base = usize::from((bgcnt >> 2) & 0x3) * CHAR_BLOCK_SIZE;
        let screen_base = usize::from((bgcnt >> 8) & 0x1f) * SCREEN_BLOCK_SIZE;
        let is_8bpp = bgcnt & (1 << 7) != 0;
        let (bg_width, bg_height) = text_bg_size(bgcnt);

        let hofs = if self.bghofs_scanline_valid[bg] {
            self.bghofs_scanline[bg][screen_y]
        } else {
            self.bghofs[bg]
        };
        let vofs = if self.bgvofs_scanline_valid[bg] {
            self.bgvofs_scanline[bg][screen_y]
        } else {
            self.bgvofs[bg]
        };
        let bg_x = (screen_x + usize::from(hofs)) % bg_width;
        let bg_y = (screen_y + usize::from(vofs)) % bg_height;
        let color_index =
            text_bg_color_index(vram, char_base, screen_base, is_8bpp, bg_width, bg_x, bg_y)?;
        if color_index == 0 {
            return None;
        }
        read_u16_checked(palette, usize::from(color_index) * 2)
    }

    fn bg_priority(&self, bg: usize) -> u16 {
        self.bgcnt[bg] & 0x3
    }

    fn sorted_bg_order(&self) -> [usize; 4] {
        let mut bgs = [0usize, 1, 2, 3];
        bgs.sort_by_key(|&bg| (self.bg_priority(bg), bg));
        bgs
    }

    fn apply_blend(&self, top: RenderedLayer, lower: Option<RenderedLayer>) -> u16 {
        let mode = (self.bldcnt >> 6) & 0x3;
        let top_targets = self.bldcnt & 0x003f;
        let lower_targets = (self.bldcnt >> 8) & 0x003f;

        if mode == 0 || top_targets & top.blend_layer == 0 {
            return top.color;
        }

        match mode {
            BLEND_MODE_ALPHA => {
                let Some(lower) = lower else {
                    return top.color;
                };
                if lower_targets & lower.blend_layer == 0 {
                    return top.color;
                }
                let eva = (self.bldalpha & 0x001f).min(16);
                let evb = ((self.bldalpha >> 8) & 0x001f).min(16);
                alpha_blend(top.color, lower.color, eva, evb)
            }
            BLEND_MODE_LIGHTEN => brightness_blend(top.color, self.bldy & 0x001f, true),
            BLEND_MODE_DARKEN => brightness_blend(top.color, self.bldy & 0x001f, false),
            _ => top.color,
        }
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
            self.winout & WIN_MASK_ALL
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
struct RenderedLayer {
    color: u16,
    blend_layer: u16,
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

#[derive(Clone, Copy)]
struct ObjAffineParams {
    pa: i16,
    pb: i16,
    pc: i16,
    pd: i16,
}

fn obj_affine_params(oam: &[u8], index: usize) -> ObjAffineParams {
    let base = index * 32;
    ObjAffineParams {
        pa: read_u16_checked(oam, base + 6).unwrap_or(0x0100) as i16,
        pb: read_u16_checked(oam, base + 14).unwrap_or(0) as i16,
        pc: read_u16_checked(oam, base + 22).unwrap_or(0) as i16,
        pd: read_u16_checked(oam, base + 30).unwrap_or(0x0100) as i16,
    }
}

fn obj_source_pixel(
    draw_x: usize,
    draw_y: usize,
    obj_width: usize,
    obj_height: usize,
    draw_width: usize,
    draw_height: usize,
    affine: Option<ObjAffineParams>,
) -> Option<(usize, usize)> {
    let Some(params) = affine else {
        return Some((draw_x, draw_y));
    };

    let x = draw_x as i32 - (draw_width as i32 / 2);
    let y = draw_y as i32 - (draw_height as i32 / 2);
    let source_x =
        ((i32::from(params.pa) * x + i32::from(params.pb) * y) >> 8) + (obj_width as i32 / 2);
    let source_y =
        ((i32::from(params.pc) * x + i32::from(params.pd) * y) >> 8) + (obj_height as i32 / 2);

    if source_x < 0 || source_y < 0 || source_x >= obj_width as i32 || source_y >= obj_height as i32
    {
        return None;
    }
    Some((source_x as usize, source_y as usize))
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
        let color = *vram.get(tile_offset)?;
        if color == 0 {
            None
        } else {
            Some(u16::from(color))
        }
    } else {
        let tile_offset = char_base + tile_number * 32 + (pixel_y * BG_TILE_SIZE + pixel_x) / 2;
        let byte = *vram.get(tile_offset)?;
        let color = if pixel_x & 1 == 0 {
            byte & 0x0f
        } else {
            byte >> 4
        };
        if color == 0 {
            None
        } else {
            Some(((entry >> 12) & 0x0f) * 16 + u16::from(color))
        }
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

fn affine_source_coord(reference: u32, x_step: u16, y_step: u16, x: usize, y: usize) -> i32 {
    let reference = sign_extend_u32(reference & 0x0fff_ffff, 28) as i64;
    let x_step = i64::from(x_step as i16);
    let y_step = i64::from(y_step as i16);
    ((reference + x_step * x as i64 + y_step * y as i64) >> 8) as i32
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

fn sign_extend_u32(value: u32, bits: u32) -> i32 {
    let sign_bit = 1u32 << (bits - 1);
    if value & sign_bit != 0 {
        value as i32 - (1 << bits)
    } else {
        value as i32
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

fn alpha_blend(top: u16, lower: u16, eva: u16, evb: u16) -> u16 {
    let r = ((((top & 0x001f) * eva) + ((lower & 0x001f) * evb)) / 16).min(31);
    let g = (((((top >> 5) & 0x001f) * eva) + (((lower >> 5) & 0x001f) * evb)) / 16).min(31);
    let b = (((((top >> 10) & 0x001f) * eva) + (((lower >> 10) & 0x001f) * evb)) / 16).min(31);
    r | (g << 5) | (b << 10)
}

fn brightness_blend(color: u16, evy: u16, lighten: bool) -> u16 {
    let evy = evy.min(16);
    let r = brightness_channel(color & 0x001f, evy, lighten);
    let g = brightness_channel((color >> 5) & 0x001f, evy, lighten);
    let b = brightness_channel((color >> 10) & 0x001f, evy, lighten);
    r | (g << 5) | (b << 10)
}

fn brightness_channel(channel: u16, evy: u16, lighten: bool) -> u16 {
    if lighten {
        channel + ((31 - channel) * evy) / 16
    } else {
        channel - (channel * evy) / 16
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

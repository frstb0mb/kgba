use super::*;
use crate::gba::ppu::{DISPSTAT_VBLANK, TOTAL_SCANLINES, VISIBLE_SCANLINES};

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
fn mode3_applies_bg2_affine_scaling() {
    let mut vram = vec![0; WIDTH * HEIGHT * 2];
    vram[0..2].copy_from_slice(&rgb5(31, 0, 0).to_le_bytes());
    vram[2..4].copy_from_slice(&rgb5(0, 31, 0).to_le_bytes());
    vram[4..6].copy_from_slice(&rgb5(0, 0, 31).to_le_bytes());

    let mut ppu = Ppu::new();
    ppu.write_dispcnt(MODE_3 | BG2_ENABLE);
    ppu.write_bgpa(2, 0x0200);
    ppu.write_bgpd(2, 0x0100);

    let frame = ppu.render_mode3(&vram);

    assert_eq!(frame[0], 0xffff0000);
    assert_eq!(frame[1], 0xff0000ff);
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
fn mode0_treats_4bpp_color_zero_as_transparent_even_with_palette_bank() {
    let mut palette = vec![0; 0x400];
    palette[2..4].copy_from_slice(&rgb5(31, 0, 0).to_le_bytes());
    palette[16 * 2..16 * 2 + 2].copy_from_slice(&rgb5(0, 31, 0).to_le_bytes());

    let mut vram = vec![0; 0x18000];
    vram[0] = 0x11;
    write_vram_halfword_offset(&mut vram, 11 * SCREEN_BLOCK_SIZE, 0);
    write_vram_halfword_offset(&mut vram, 12 * SCREEN_BLOCK_SIZE, 1 << 12);

    let mut ppu = Ppu::new();
    ppu.write_dispcnt(MODE_0 | BG0_ENABLE | BG1_ENABLE);
    ppu.write_bgcnt(0, 11 << 8);
    ppu.write_bgcnt(1, (2 << 2) | (12 << 8));

    let frame = ppu.render_mode0(&palette, &vram);

    assert_eq!(frame[0], 0xffff0000);
}

#[test]
fn mode0_alpha_blends_top_and_lower_targets() {
    let mut palette = vec![0; 0x400];
    palette[2..4].copy_from_slice(&rgb5(31, 0, 0).to_le_bytes());
    palette[16 * 2 + 2..16 * 2 + 4].copy_from_slice(&rgb5(0, 0, 31).to_le_bytes());

    let mut vram = vec![0; 0x18000];
    vram[0] = 0x11;
    vram[2 * CHAR_BLOCK_SIZE] = 0x11;
    write_vram_halfword_offset(&mut vram, 11 * SCREEN_BLOCK_SIZE, 0);
    write_vram_halfword_offset(&mut vram, 12 * SCREEN_BLOCK_SIZE, 1 << 12);

    let mut ppu = Ppu::new();
    ppu.write_dispcnt(MODE_0 | BG0_ENABLE | BG1_ENABLE);
    ppu.write_bgcnt(0, 11 << 8);
    ppu.write_bgcnt(1, (2 << 2) | (12 << 8));
    ppu.write_bldcnt(BLEND_LAYER_BG[0] | (BLEND_MODE_ALPHA << 6) | (BLEND_LAYER_BG[1] << 8));
    ppu.write_bldalpha(8 | (8 << 8));

    let frame = ppu.render_mode0(&palette, &vram);

    assert_eq!(frame[0], bgr555_to_argb8888(rgb5(15, 0, 15)));
}

#[test]
fn mode0_does_not_alpha_blend_when_lower_layer_is_not_targeted() {
    let mut palette = vec![0; 0x400];
    palette[2..4].copy_from_slice(&rgb5(31, 0, 0).to_le_bytes());
    palette[16 * 2 + 2..16 * 2 + 4].copy_from_slice(&rgb5(0, 0, 31).to_le_bytes());

    let mut vram = vec![0; 0x18000];
    vram[0] = 0x11;
    vram[2 * CHAR_BLOCK_SIZE] = 0x11;
    write_vram_halfword_offset(&mut vram, 11 * SCREEN_BLOCK_SIZE, 0);
    write_vram_halfword_offset(&mut vram, 12 * SCREEN_BLOCK_SIZE, 1 << 12);

    let mut ppu = Ppu::new();
    ppu.write_dispcnt(MODE_0 | BG0_ENABLE | BG1_ENABLE);
    ppu.write_bgcnt(0, 11 << 8);
    ppu.write_bgcnt(1, (2 << 2) | (12 << 8));
    ppu.write_bldcnt(BLEND_LAYER_BG[0] | (BLEND_MODE_ALPHA << 6));
    ppu.write_bldalpha(8 | (8 << 8));

    let frame = ppu.render_mode0(&palette, &vram);

    assert_eq!(frame[0], 0xffff0000);
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

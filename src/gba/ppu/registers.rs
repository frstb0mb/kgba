use super::{
    DISPSTAT_HBLANK, DISPSTAT_IRQ_WRITABLE_MASK, DISPSTAT_VBLANK, DISPSTAT_VCOUNT,
    DISPSTAT_VCOUNT_SETTING_MASK, TOTAL_SCANLINES, VISIBLE_SCANLINES,
};

#[derive(Debug)]
pub struct Ppu {
    pub(super) dispcnt: u16,
    pub(super) dispstat: u16,
    pub(super) vcount: u16,
    pub(super) bgcnt: [u16; 4],
    pub(super) bghofs: [u16; 4],
    pub(super) bgvofs: [u16; 4],
    pub(super) bgpa: [u16; 2],
    pub(super) bgpb: [u16; 2],
    pub(super) bgpc: [u16; 2],
    pub(super) bgpd: [u16; 2],
    pub(super) bgx: [u32; 2],
    pub(super) bgy: [u32; 2],
    pub(super) winh: [u16; 2],
    pub(super) winv: [u16; 2],
    pub(super) winin: u16,
    pub(super) winout: u16,
    pub(super) mosaic: u16,
    pub(super) bldcnt: u16,
    pub(super) bldalpha: u16,
    pub(super) bldy: u16,
}

impl Default for Ppu {
    fn default() -> Self {
        Self {
            dispcnt: 0,
            dispstat: 0,
            vcount: 0,
            bgcnt: [0; 4],
            bghofs: [0; 4],
            bgvofs: [0; 4],
            bgpa: [0x0100; 2],
            bgpb: [0; 2],
            bgpc: [0; 2],
            bgpd: [0x0100; 2],
            bgx: [0; 2],
            bgy: [0; 2],
            winh: [0; 2],
            winv: [0; 2],
            winin: 0,
            winout: 0,
            mosaic: 0,
            bldcnt: 0,
            bldalpha: 0,
            bldy: 0,
        }
    }
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

    pub fn bgpa(&self, bg: usize) -> u16 {
        self.bgpa[bg - 2]
    }

    pub fn bgpb(&self, bg: usize) -> u16 {
        self.bgpb[bg - 2]
    }

    pub fn bgpc(&self, bg: usize) -> u16 {
        self.bgpc[bg - 2]
    }

    pub fn bgpd(&self, bg: usize) -> u16 {
        self.bgpd[bg - 2]
    }

    pub fn bgx(&self, bg: usize) -> u32 {
        self.bgx[bg - 2]
    }

    pub fn bgy(&self, bg: usize) -> u32 {
        self.bgy[bg - 2]
    }

    pub fn mosaic(&self) -> u16 {
        self.mosaic
    }

    pub fn bldcnt(&self) -> u16 {
        self.bldcnt
    }

    pub fn bldalpha(&self) -> u16 {
        self.bldalpha
    }

    pub fn bldy(&self) -> u16 {
        self.bldy
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

    pub fn write_bgpa(&mut self, bg: usize, value: u16) {
        self.bgpa[bg - 2] = value;
    }

    pub fn write_bgpb(&mut self, bg: usize, value: u16) {
        self.bgpb[bg - 2] = value;
    }

    pub fn write_bgpc(&mut self, bg: usize, value: u16) {
        self.bgpc[bg - 2] = value;
    }

    pub fn write_bgpd(&mut self, bg: usize, value: u16) {
        self.bgpd[bg - 2] = value;
    }

    pub fn write_bgx(&mut self, bg: usize, value: u32) {
        self.bgx[bg - 2] = value & 0x0fff_ffff;
    }

    pub fn write_bgy(&mut self, bg: usize, value: u32) {
        self.bgy[bg - 2] = value & 0x0fff_ffff;
    }

    pub fn write_bgx_halfword(&mut self, bg: usize, halfword: usize, value: u16) {
        let index = bg - 2;
        let mut current = self.bgx[index];
        if halfword == 0 {
            current = (current & 0xffff_0000) | u32::from(value);
        } else {
            current = (current & 0x0000_ffff) | (u32::from(value) << 16);
        }
        self.write_bgx(bg, current);
    }

    pub fn write_bgy_halfword(&mut self, bg: usize, halfword: usize, value: u16) {
        let index = bg - 2;
        let mut current = self.bgy[index];
        if halfword == 0 {
            current = (current & 0xffff_0000) | u32::from(value);
        } else {
            current = (current & 0x0000_ffff) | (u32::from(value) << 16);
        }
        self.write_bgy(bg, current);
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

    pub fn write_bldcnt(&mut self, value: u16) {
        self.bldcnt = value & 0x3fff;
    }

    pub fn write_bldalpha(&mut self, value: u16) {
        self.bldalpha = value & 0x1f1f;
    }

    pub fn write_bldy(&mut self, value: u16) {
        self.bldy = value & 0x001f;
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
}

use super::{
    bus::{AccessSize, Bus},
    cartridge::Cartridge,
    memory_map::{
        DISPCNT, EWRAM_START, GAME_PAK_ROM_START, IO_START, IWRAM_START, VCOUNT, VRAM_START,
    },
};

const DEFAULT_MAX_INSTRUCTIONS: usize = 200_000;

#[derive(Debug)]
pub struct SoftwareRunner {
    regs: [u32; 16],
    thumb: bool,
    instructions: usize,
    vcount_reads: u32,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RunResult {
    FrameReady,
    InstructionLimit { pc: u32 },
}

impl SoftwareRunner {
    pub fn new_for_rom(cartridge: &Cartridge) -> Self {
        let mut regs = [0; 16];
        regs[13] = IWRAM_START + 0x7f00;

        let (pc, thumb) = if let Some(main) = cartridge.entry_thumb_addr() {
            (main, true)
        } else {
            (GAME_PAK_ROM_START, false)
        };
        regs[15] = pc;

        Self {
            regs,
            thumb,
            instructions: 0,
            vcount_reads: 0,
        }
    }

    pub fn run_until_frame(
        &mut self,
        cartridge: &Cartridge,
        bus: &mut Bus<'_>,
    ) -> Result<RunResult, String> {
        for _ in 0..DEFAULT_MAX_INSTRUCTIONS {
            self.instructions += 1;
            if self.thumb {
                self.step_thumb(cartridge, bus)?;
            } else {
                self.step_arm(cartridge, bus)?;
            }

            if self.instructions > 100 && self.regs[15] >= GAME_PAK_ROM_START + 0x25e {
                return Ok(RunResult::FrameReady);
            }
        }
        Ok(RunResult::InstructionLimit { pc: self.regs[15] })
    }

    fn step_arm(&mut self, cartridge: &Cartridge, bus: &mut Bus<'_>) -> Result<(), String> {
        let pc = self.regs[15];
        let op = cartridge.read_u32(pc);
        self.regs[15] = pc.wrapping_add(4);

        if (op & 0x0f00_0000) == 0x0a00_0000 {
            let mut offset = op & 0x00ff_ffff;
            if offset & 0x0080_0000 != 0 {
                offset |= 0xff00_0000;
            }
            self.regs[15] = pc
                .wrapping_add(8)
                .wrapping_add(((offset as i32) << 2) as u32);
            return Ok(());
        }

        match op {
            0xe3a0_0301 => self.regs[0] = 0x0400_0000,
            0xe580_0208 => self.write_u32(self.regs[0].wrapping_add(0x208), self.regs[0], bus),
            0xe3a0_0012 => self.regs[0] = 0x12,
            0xe3a0_001f => self.regs[0] = 0x1f,
            0xe129_f000 => {}
            0xe59f_d0ac | 0xe59f_d0a4 => self.regs[13] = cartridge.read_u32(pc + 8 + (op & 0xfff)),
            0xe28f_0001 => self.regs[0] = pc + 8 + 1,
            0xe12f_ff10 => self.branch_exchange(self.regs[0]),
            _ => return Err(format!("unsupported ARM op {op:#010x} at {pc:#010x}")),
        }
        Ok(())
    }

    fn step_thumb(&mut self, cartridge: &Cartridge, bus: &mut Bus<'_>) -> Result<(), String> {
        let pc = self.regs[15];
        let op = cartridge.read_u16(pc);
        self.regs[15] = pc.wrapping_add(2);

        match op {
            0x46c0 => {}
            op if (op & 0xf800) == 0x2000 => {
                let rd = ((op >> 8) & 0x7) as usize;
                self.regs[rd] = u32::from(op & 0xff);
            }
            op if (op & 0xf800) == 0x4800 => {
                let rd = ((op >> 8) & 0x7) as usize;
                let imm = u32::from(op & 0xff) * 4;
                let addr = ((pc + 4) & !3).wrapping_add(imm);
                self.regs[rd] = cartridge.read_u32(addr);
            }
            op if (op & 0xf800) == 0x0000 => {
                let rd = (op & 0x7) as usize;
                let rm = ((op >> 3) & 0x7) as usize;
                let imm = (op >> 6) & 0x1f;
                self.regs[rd] = self.regs[rm] << imm;
            }
            op if (op & 0xf800) == 0x0800 => {
                let rd = (op & 0x7) as usize;
                let rm = ((op >> 3) & 0x7) as usize;
                let imm = (op >> 6) & 0x1f;
                let imm = if imm == 0 { 32 } else { imm };
                self.regs[rd] = self.regs[rm] >> imm;
            }
            op if (op & 0xfe00) == 0x1800 => {
                let rd = (op & 0x7) as usize;
                let rn = ((op >> 3) & 0x7) as usize;
                let rm = ((op >> 6) & 0x7) as usize;
                self.regs[rd] = self.regs[rn].wrapping_add(self.regs[rm]);
            }
            op if (op & 0xfe00) == 0x1a00 => {
                let rd = (op & 0x7) as usize;
                let rn = ((op >> 3) & 0x7) as usize;
                let rm = ((op >> 6) & 0x7) as usize;
                self.regs[rd] = self.regs[rn].wrapping_sub(self.regs[rm]);
            }
            op if (op & 0xf800) == 0x3000 => {
                let rd = ((op >> 8) & 0x7) as usize;
                self.regs[rd] = self.regs[rd].wrapping_add(u32::from(op & 0xff));
            }
            op if (op & 0xf800) == 0x3800 => {
                let rd = ((op >> 8) & 0x7) as usize;
                self.regs[rd] = self.regs[rd].wrapping_sub(u32::from(op & 0xff));
            }
            op if (op & 0xf800) == 0x2800 => {
                let rn = ((op >> 8) & 0x7) as usize;
                let rhs = u32::from(op & 0xff);
                self.set_cmp(self.regs[rn], rhs);
            }
            op if (op & 0xf800) == 0x8000 => {
                let rb = ((op >> 3) & 0x7) as usize;
                let rd = (op & 0x7) as usize;
                let addr = self.regs[rb].wrapping_add(u32::from((op >> 6) & 0x1f) * 2);
                self.write_u16(addr, self.regs[rd] as u16, bus);
            }
            op if (op & 0xfe00) == 0x5200 => {
                let ro = ((op >> 6) & 0x7) as usize;
                let rb = ((op >> 3) & 0x7) as usize;
                let rd = (op & 0x7) as usize;
                let addr = self.regs[rb].wrapping_add(self.regs[ro]);
                self.write_u16(addr, self.regs[rd] as u16, bus);
            }
            op if (op & 0xf800) == 0x8800 => {
                let rb = ((op >> 3) & 0x7) as usize;
                let rd = (op & 0x7) as usize;
                let addr = self.regs[rb].wrapping_add(u32::from((op >> 6) & 0x1f) * 2);
                self.regs[rd] = u32::from(self.read_u16(addr, bus));
            }
            op if (op & 0xfc00) == 0x4000 => self.step_alu(op)?,
            op if (op & 0xfc00) == 0x4400 => self.step_high_register(op),
            op if (op & 0xff00) == 0xb500 => self.push(op),
            0xb500..=0xb5ff => self.push(op),
            op if (op & 0xf000) == 0xd000 => self.conditional_branch(op),
            op if (op & 0xf800) == 0xe000 => {
                let mut offset = u32::from(op & 0x07ff);
                if offset & 0x400 != 0 {
                    offset |= 0xffff_f800;
                }
                self.regs[15] = self.regs[15].wrapping_add(((offset as i32) << 1) as u32);
            }
            0xdf00 => return Ok(()),
            _ => return Err(format!("unsupported Thumb op {op:#06x} at {pc:#010x}")),
        }

        Ok(())
    }

    fn step_alu(&mut self, op: u16) -> Result<(), String> {
        let rd = (op & 0x7) as usize;
        let rs = ((op >> 3) & 0x7) as usize;
        match (op >> 6) & 0xf {
            0x8 => self.set_cmp(self.regs[rd], self.regs[rs]),
            0xa => self.set_cmp(self.regs[rd], self.regs[rs]),
            0xe => self.regs[rd] &= !self.regs[rs],
            _ => return Err(format!("unsupported Thumb ALU op {op:#06x}")),
        }
        Ok(())
    }

    fn step_high_register(&mut self, op: u16) {
        let h1 = ((op >> 7) & 1) as usize;
        let h2 = ((op >> 6) & 1) as usize;
        let rd = ((op & 0x7) as usize) + h1 * 8;
        let rs = (((op >> 3) & 0x7) as usize) + h2 * 8;
        match (op >> 8) & 0x3 {
            0x0 => self.regs[rd] = self.regs[rd].wrapping_add(self.regs[rs]),
            0x2 => self.regs[rd] = self.regs[rs],
            0x3 => self.branch_exchange(self.regs[rs]),
            _ => {}
        }
    }

    fn conditional_branch(&mut self, op: u16) {
        let cond = (op >> 8) & 0xf;
        let take = match cond {
            0x0 => self.z(),
            0x1 => !self.z(),
            0x8 => self.c() && !self.z(),
            0x9 => !self.c() || self.z(),
            _ => false,
        };
        if take {
            let mut offset = u32::from(op & 0xff);
            if offset & 0x80 != 0 {
                offset |= 0xffff_ff00;
            }
            self.regs[15] = self.regs[15].wrapping_add(((offset as i32) << 1) as u32);
        }
    }

    fn push(&mut self, op: u16) {
        let mut count = (op & 0xff).count_ones();
        if op & 0x0100 != 0 {
            count += 1;
        }
        self.regs[13] = self.regs[13].wrapping_sub(count * 4);
    }

    fn read_u16(&mut self, addr: u32, bus: &mut Bus<'_>) -> u16 {
        if addr == VCOUNT {
            self.vcount_reads += 1;
            bus.ppu_mut()
                .set_vcount(if self.vcount_reads < 3 { 0 } else { 160 });
            return bus.read(addr, AccessSize::Halfword) as u16;
        }
        0
    }

    fn write_u16(&mut self, addr: u32, value: u16, bus: &mut Bus<'_>) {
        if addr == DISPCNT {
            bus.write(addr, AccessSize::Halfword, u32::from(value));
        } else if (VRAM_START..VRAM_START + 0x18000).contains(&addr) {
            let offset = (addr - VRAM_START) as usize;
            bus.memory_mut().write_vram_halfword(offset, value);
        }
    }

    fn write_u32(&mut self, addr: u32, value: u32, bus: &mut Bus<'_>) {
        if (IO_START..IO_START + 0x400).contains(&addr) {
            bus.write(addr, AccessSize::Word, value);
        } else if (EWRAM_START..EWRAM_START + 0x40000).contains(&addr)
            || (IWRAM_START..IWRAM_START + 0x8000).contains(&addr)
        {
        }
    }

    fn branch_exchange(&mut self, value: u32) {
        self.thumb = value & 1 != 0;
        self.regs[15] = value & !1;
    }

    fn set_cmp(&mut self, lhs: u32, rhs: u32) {
        let result = lhs.wrapping_sub(rhs);
        self.regs[14] = 0;
        if result == 0 {
            self.regs[14] |= 1 << 30;
        }
        if lhs >= rhs {
            self.regs[14] |= 1 << 29;
        }
    }

    fn z(&self) -> bool {
        self.regs[14] & (1 << 30) != 0
    }

    fn c(&self) -> bool {
        self.regs[14] & (1 << 29) != 0
    }
}

use crate::gba::memory_map::IO_START;

use super::{memory::MemoryRegion, trace::trace_timer_counter, util::clean_dcache_area};

#[derive(Debug, Default)]
pub struct Timers {
    timers: [Timer; 4],
}

impl Timers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write_register(&mut self, addr: u32, value: u16, io: &MemoryRegion) {
        let relative = addr - (IO_START + 0x0100);
        let timer_index = (relative / 4) as usize;
        if timer_index >= self.timers.len() {
            return;
        }

        if relative & 0x2 == 0 {
            self.timers[timer_index].reload = value;
        } else {
            let was_enabled = self.timers[timer_index].enabled();
            self.timers[timer_index].control = value;
            self.timers[timer_index].accumulated_cycles = 0;
            if !was_enabled && self.timers[timer_index].enabled() {
                self.timers[timer_index].counter = self.timers[timer_index].reload;
                write_timer_counter(io, timer_index, self.timers[timer_index].counter);
            }
        }
    }

    pub fn advance(&mut self, cycles: u32, io: &MemoryRegion) -> [u32; 4] {
        let overflow0 = self.advance_timer(0, cycles, io);
        let overflow1 = if self.timers[1].cascade() {
            self.advance_cascade_timer(1, overflow0, io)
        } else {
            self.advance_timer(1, cycles, io)
        };
        let overflow2 = if self.timers[2].cascade() {
            self.advance_cascade_timer(2, overflow1, io)
        } else {
            self.advance_timer(2, cycles, io)
        };
        let overflow3 = if self.timers[3].cascade() {
            self.advance_cascade_timer(3, overflow2, io)
        } else {
            self.advance_timer(3, cycles, io)
        };
        [overflow0, overflow1, overflow2, overflow3]
    }

    fn advance_timer(&mut self, timer_index: usize, cycles: u32, io: &MemoryRegion) -> u32 {
        let timer = &mut self.timers[timer_index];
        if !timer.enabled() || timer.cascade() {
            return 0;
        }

        timer.accumulated_cycles += u64::from(cycles);
        let period = u64::from(timer.period_cycles());
        let ticks = (timer.accumulated_cycles / period) as u32;
        timer.accumulated_cycles %= period;
        self.add_ticks(timer_index, ticks, io)
    }

    fn advance_cascade_timer(&mut self, timer_index: usize, ticks: u32, io: &MemoryRegion) -> u32 {
        let timer = &self.timers[timer_index];
        if !timer.enabled() || !timer.cascade() {
            return 0;
        }
        self.add_ticks(timer_index, ticks, io)
    }

    fn add_ticks(&mut self, timer_index: usize, ticks: u32, io: &MemoryRegion) -> u32 {
        let mut overflows = 0;
        for _ in 0..ticks {
            let timer = &mut self.timers[timer_index];
            let (next, overflow) = timer.counter.overflowing_add(1);
            if overflow {
                timer.counter = timer.reload;
                overflows += 1;
            } else {
                timer.counter = next;
            }
        }
        write_timer_counter(io, timer_index, self.timers[timer_index].counter);
        trace_timer_counter(
            timer_index,
            self.timers[timer_index].counter,
            ticks,
            overflows,
        );
        overflows
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Timer {
    reload: u16,
    counter: u16,
    control: u16,
    accumulated_cycles: u64,
}

impl Timer {
    fn enabled(self) -> bool {
        self.control & (1 << 7) != 0
    }

    fn cascade(self) -> bool {
        self.control & (1 << 2) != 0
    }

    fn period_cycles(self) -> u32 {
        match self.control & 0x3 {
            0 => 1,
            1 => 64,
            2 => 256,
            3 => 1024,
            _ => unreachable!(),
        }
    }
}

fn write_timer_counter(io: &MemoryRegion, timer_index: usize, value: u16) {
    let offset = 0x0100 + timer_index * 4;
    let bytes = value.to_le_bytes();
    let io = io.as_mut_slice();
    io[offset] = bytes[0];
    io[offset + 1] = bytes[1];
    clean_dcache_area(io.as_mut_ptr().wrapping_add(offset), 2);
}
